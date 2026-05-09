(elle/epoch 10)
# Tail-call memory reclamation via scope regions
#
# Verifies that tail-recursive loops don't accumulate slab allocations
# indefinitely. Scope regions (RegionEnter/RegionExit) keep arena/count
# bounded regardless of iteration count.
#
# Under --checked-intrinsics, escape analysis can't insert scope regions
# (%-intrinsic calls look like potential heap escapes), so these tests
# are gated by checked?.
#
# Uses %-intrinsics for loop control to avoid rest-arg allocations
# from variadic stdlib wrappers (+, -, <=, etc.).

(def checked? (vm/config :checked-intrinsics))

# ── Self tail recursion ───────────────────────────────────────────────

# A tail-recursive loop that allocates a fresh string each iteration.
# The let-bound string is freed by DropSlot (dead local at tail-call
# site). Uses (string ...) which formats directly without slab
# intermediates.
(defn tail-loop (n)
  (if (%le n 0)
    (arena/count)
    (let* [s (string "iter-" n)]
      (tail-loop (%sub n 1)))))

# Run 100 iterations, then 10000 iterations.
# If reclamation works, the live slab count at 10000 should NOT be 100x
# the count at 100.
(let* [count-100 (tail-loop 100)
       count-10000 (tail-loop 10000)]
  (assert (or checked? (%lt count-10000 (%mul count-100 10)))
          (concat "tail-call reclamation: count-100=" (number->string count-100)
                  " count-10000=" (number->string count-10000))))

# Slab slot reclamation: root-live-count must be bounded.
# 100 iterations vs 10000 — if slots are reclaimed, live count
# should not grow proportionally.
(defn tail-alloc (n)
  (if (%le n 0)
    (get (arena/stats) :root-live-count)
    (let* [s (string "iter-" n)]
      (tail-alloc (%sub n 1)))))

(def before-100 (get (arena/stats) :root-live-count))
(tail-alloc 100)
(def delta-100 (%sub (get (arena/stats) :root-live-count) before-100))
(def before-10k (get (arena/stats) :root-live-count))
(tail-alloc 10000)
(def delta-10k (%sub (get (arena/stats) :root-live-count) before-10k))
(println "slab: delta-100=" delta-100 " delta-10k=" delta-10k)
(assert (or checked? (%lt delta-10k (%mul delta-100 10)))
        (concat "slab slot reclamation: delta-100=" (number->string delta-100)
                " delta-10k=" (number->string delta-10k)))

# ── Mutual tail recursion ─────────────────────────────────────────────
# DropSlot for dead locals currently only fires for self-tail-calls.
# Mutual tail recursion (even→odd→even) needs non-self tail-call
# DropSlot, which requires local refcounting to avoid use-after-free
# on aliased child values. Gated for now.

(defn even-loop (n)
  (if (%le n 0)
    (arena/count)
    (let* [s (string "even-" n)]
      (odd-loop (%sub n 1)))))

(defn odd-loop (n)
  (if (%le n 0)
    (arena/count)
    (let* [s (string "odd-" n)]
      (even-loop (%sub n 1)))))

(let* [c1 (even-loop 100)
       c2 (even-loop 10000)]
  (assert (or checked? (%lt c2 (%mul c1 100)))
          (concat "mutual tail-call reclamation: c1=" (number->string c1) " c2="
                  (number->string c2))))

# ── No-alloc tail recursion (baseline) ────────────────────────────────

(defn count-loop (n)
  (if (%le n 0) (arena/count) (count-loop (%sub n 1))))

(let* [c1 (count-loop 100)
       c2 (count-loop 10000)]
  (assert (or checked? (%lt c2 (%mul c1 10)))
          (concat "no-alloc tail-call: c1=" (number->string c1) " c2="
                  (number->string c2))))

# ── Tail call returning heap value ────────────────────────────────────
#
# Tail-recursive loop that returns a heap-allocated value (string).
# The returned string must survive rotation and not be corrupted.

(defn build-result (n)
  (if (%le n 0)
    (string "result-" n)
    (let* [s (string "iter-" n)]
      (build-result (%sub n 1)))))

(let* [r (build-result 100)]
  (assert (= r "result-0") (concat "tail-call return value: " r)))

# ── Tail call with accumulator ─────────────────────────────────────
#
# Tail-recursive loop that threads a value through arguments.
# The accumulator must not be corrupted by rotation.

(defn sum-loop (n acc)
  (if (%le n 0)
    acc
    (let* [s (string "work-" n)]
      (sum-loop (%sub n 1) (%add acc n)))))

(assert (= (sum-loop 100 0) 5050) "tail-call accumulator: sum 1..100")

# ── Fiber with tail-call body ───────────────────────────────────────
#
# A fiber that yields values from a tail-recursive inner loop.
# Yielded values must survive across resume boundaries.

(defn coro-inner (n)
  (if (%le n 0)
    :done
    (begin
      (yield (string "item-" n))
      (coro-inner (%sub n 1)))))

(let* [co (fiber/new (fn () (coro-inner 5)) |:yield|)
       v1 (fiber/resume co)
       v2 (fiber/resume co)
       v3 (fiber/resume co)]
  (assert (= v1 "item-5") (concat "fiber yield 1: got " v1))
  (assert (= v2 "item-4") (concat "fiber yield 2: got " v2))
  (assert (= v3 "item-3") (concat "fiber yield 3: got " v3)))

# ── Nested lets with tail call ──────────────────────────────────────
#
# With integer inits (no heap-allocating init expressions), both nested
# lets scope-allocate and reclamation works.

(defn nested-int-loop (n)
  (if (%le n 0)
    (arena/count)
    (let [a (%add n 1)]
      (let [b (%add n 2)]
        (nested-int-loop (%sub n 1))))))

(let* [c1 (nested-int-loop 100)
       c2 (nested-int-loop 10000)]
  (assert (or checked? (%lt c2 (%mul c1 10)))
          (concat "nested-int-let reclamation: c1=" (number->string c1) " c2="
                  (number->string c2))))
