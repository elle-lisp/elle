(elle/epoch 9)
# Tail-call memory reclamation via pool rotation
#
# Verifies that tail-recursive loops don't accumulate slab allocations
# indefinitely. The two-pool rotation should keep arena/count bounded
# regardless of iteration count.

# ── Self tail recursion ───────────────────────────────────────────────

# A tail-recursive loop that allocates a fresh string each iteration.
# Without pool rotation, arena/count grows linearly with N.
# With rotation, it stays bounded (~2x per-iteration working set).
(defn tail-loop (n)
  (if (<= n 0)
    (arena/count)
    (let* [s (concat "iter-" (number->string n))]
      (tail-loop (- n 1)))))

# Run 100 iterations, then 10000 iterations.
# If reclamation works, the count at 10000 should NOT be 100x the count at 100.
(let* [count-100 (tail-loop 100)
       count-10000 (tail-loop 10000)]
  (assert (< count-10000 (* count-100 10))
    (concat "tail-call reclamation: count-100=" (number->string count-100)
      " count-10000=" (number->string count-10000))))

# ── Mutual tail recursion ─────────────────────────────────────────────

(defn even-loop (n)
  (if (<= n 0)
    (arena/count)
    (let* [s (concat "even-" (number->string n))]
      (odd-loop (- n 1)))))

(defn odd-loop (n)
  (if (<= n 0)
    (arena/count)
    (let* [s (concat "odd-" (number->string n))]
      (even-loop (- n 1)))))

(let* [c1 (even-loop 100)
       c2 (even-loop 10000)]
  (assert (< c2 (* c1 10))
    (concat "mutual tail-call reclamation: c1=" (number->string c1) " c2="
      (number->string c2))))

# ── No-alloc tail recursion (baseline) ────────────────────────────────

(defn count-loop (n)
  (if (<= n 0) (arena/count) (count-loop (- n 1))))

(let* [c1 (count-loop 100)
       c2 (count-loop 10000)]
  (assert (< c2 (* c1 10))
    (concat "no-alloc tail-call: c1=" (number->string c1) " c2="
      (number->string c2))))

# ── Tail call returning heap value ────────────────────────────────────
#
# Tail-recursive loop that returns a heap-allocated value (string).
# The returned string must survive rotation and not be corrupted.

(defn build-result (n)
  (if (<= n 0)
    (concat "result-" (number->string n))
    (let* [s (concat "iter-" (number->string n))]
      (build-result (- n 1)))))

(let* [r (build-result 100)]
  (assert (= r "result-0") (concat "tail-call return value: " r)))

# ── Tail call with accumulator ─────────────────────────────────────
#
# Tail-recursive loop that threads a value through arguments.
# The accumulator must not be corrupted by rotation.

(defn sum-loop (n acc)
  (if (<= n 0)
    acc
    (let* [s (concat "work-" (number->string n))]
      (sum-loop (- n 1) (+ acc n)))))

(assert (= (sum-loop 100 0) 5050) "tail-call accumulator: sum 1..100")

# ── Coroutine with tail-call body ───────────────────────────────────
#
# A coroutine that yields values from a tail-recursive inner loop.
# Yielded values must survive across resume boundaries.

(defn coro-inner (n)
  (if (<= n 0)
    :done
    (begin
      (yield (concat "item-" (number->string n)))
      (coro-inner (- n 1)))))

(let* [co (coro/new (fn () (coro-inner 5)))
       v1 (coro/resume co)
       v2 (coro/resume co)
       v3 (coro/resume co)]
  (assert (= v1 "item-5") (concat "coroutine yield 1: got " v1))
  (assert (= v2 "item-4") (concat "coroutine yield 2: got " v2))
  (assert (= v3 "item-3") (concat "coroutine yield 3: got " v3)))

# ── Nested lets with tail call ──────────────────────────────────────
#
# With integer inits (no heap-allocating init expressions), both nested
# lets scope-allocate and reclamation works.

(defn nested-int-loop (n)
  (if (<= n 0)
    (arena/count)
    (let [a (+ n 1)]
      (let [b (+ n 2)]
        (nested-int-loop (- n 1))))))

(let* [c1 (nested-int-loop 100)
       c2 (nested-int-loop 10000)]
  (assert (< c2 (* c1 10))
    (concat "nested-int-let reclamation: c1=" (number->string c1) " c2="
      (number->string c2))))
