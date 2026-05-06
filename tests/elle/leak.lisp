(elle/epoch 10)
# Memory leak test suite
#
# Verifies that heap allocations stay bounded across the runtime's
# reclamation mechanisms: scope regions, trampoline rotation, and
# flip rotation. Tests at two scales (N=100, N=10000) so linear
# leaks are caught even when absolute counts are small.
#
# Each test uses heap-allocating operations (structs, strings) that
# register in arena/count.

# ── helpers ───────────────────────────────────────────────────────

# Under --checked-intrinsics, escape analysis sets outward_heap_set=true
# (Call instructions to %-NativeFns look like potential heap escapes),
# which prevents scope region insertion. Allocation counts grow linearly
# instead of staying bounded. The tests still run (smoke) but skip the
# bounded assertion.
(def checked? (vm/config :checked-intrinsics))

(defn bounded? [d100 d10k limit]
  "True if both deltas are under limit and 10000 is not 100x 100."
  (and (%lt d100 limit) (%lt d10k limit) (or (= d100 0) (%lt d10k (* d100 10)))))

(defn linear? [d100 d1000]
  "True if growth is roughly linear (d1000 ≥ 5x d100)."
  (and (%ge d100 50) (%ge d1000 (* d100 5))))

# ── Tier 0: scope reclamation in while loops ─────────────────────
# Let-bound structs inside a while body are reclaimed by region-exit.

(defn t0-let-struct [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (let [x {:iter i}]
      x)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t0-let-struct 100)
      d10k (t0-let-struct 10000)]
  (assert (or checked? (bounded? d100 d10k 10))
          (string "t0 let-struct: d100=" d100 " d10k=" d10k)))

# Discarded struct (no let binding) — scope still reclaims.

(defn t0-discard-struct [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    {:x i :y (%add i 1)}
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t0-discard-struct 100)
      d10k (t0-discard-struct 10000)]
  (assert (or checked? (bounded? d100 d10k 10))
          (string "t0 discard-struct: d100=" d100 " d10k=" d10k)))

# String allocation in while loop — scope reclaims.

(defn t0-string [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (string "iter-" i)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t0-string 100)
      d10k (t0-string 10000)]
  (assert (or checked? (bounded? d100 d10k 10))
          (string "t0 string: d100=" d100 " d10k=" d10k)))

# Pair (cons cell) allocation in while loop — scope reclaims.
# pair is a stdlib wrapper around %pair; it must be recognized
# as non-escaping so scope marks are inserted.

(defn t0-pair [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (pair i (list))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t0-pair 100)
      d10k (t0-pair 10000)]
  (assert (or checked? (bounded? d100 d10k 10))
          (string "t0 pair: d100=" d100 " d10k=" d10k)))

# Pair with let binding — scope reclaims.

(defn t0-let-pair [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (let [x (pair i ())]
      x)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t0-let-pair 100)
      d10k (t0-let-pair 10000)]
  (assert (or checked? (bounded? d100 d10k 10))
          (string "t0 let-pair: d100=" d100 " d10k=" d10k)))

# ── Tier 1: nested while loops ───────────────────────────────────
# Inner and outer loops both allocate; scoping must handle both.

(defn t1-nested [outer inner]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i outer)
    (def @j 0)
    (while (%lt j inner)
      {:x i :y j}
      (assign j (%add j 1)))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d-small (t1-nested 10 10)
      d-big (t1-nested 100 100)]
  (assert (or checked? (%lt d-small 20)) (string "t1 nested small: " d-small))
  (assert (or checked? (%lt d-big 20)) (string "t1 nested big: " d-big)))

# ── Tier 2: tail-call rotation ───────────────────────────────────
# Tail-recursive loop allocating a struct per iteration. Trampoline
# rotation + flip keep alloc_count bounded.

(defn t2-struct [n]
  (if (= n 0)
    (arena/count)
    (begin
      {:x n}
      (t2-struct (%sub n 1)))))

(let* [b1 (arena/count)
       a1 (t2-struct 100)
       d100 (%sub a1 b1)
       b2 (arena/count)
       a2 (t2-struct 10000)
       d10k (%sub a2 b2)]
  (assert (or checked? (bounded? d100 d10k 10))
          (string "t2 struct: d100=" d100 " d10k=" d10k)))

# Tail-recursive string allocation.

(defn t2-string [n]
  (if (= n 0)
    (arena/count)
    (begin
      (string "iter-" n)
      (t2-string (%sub n 1)))))

(let* [b1 (arena/count)
       a1 (t2-string 100)
       d100 (%sub a1 b1)
       b2 (arena/count)
       a2 (t2-string 10000)
       d10k (%sub a2 b2)]
  (assert (or checked? (bounded? d100 d10k 10))
          (string "t2 string: d100=" d100 " d10k=" d10k)))

# Mutual tail recursion with struct allocation.

(defn t2-even [n]
  (if (= n 0)
    (arena/count)
    (begin
      {:parity :even :n n}
      (t2-odd (%sub n 1)))))

(defn t2-odd [n]
  (if (= n 0)
    (arena/count)
    (begin
      {:parity :odd :n n}
      (t2-even (%sub n 1)))))

(let* [b1 (arena/count)
       a1 (t2-even 100)
       d100 (%sub a1 b1)
       b2 (arena/count)
       a2 (t2-even 10000)
       d10k (%sub a2 b2)]
  (assert (or checked? (bounded? d100 d10k 10))
          (string "t2 mutual: d100=" d100 " d10k=" d10k)))

# ── Tier 3: yielding while loops (flip is essential) ─────────────
# A fiber yields mid-iteration, so scope regions cannot reclaim —
# the fiber suspends before region-exit. Without flip, allocations
# accumulate linearly. With flip, FlipSwap at the back-edge rotates
# pools each iteration.

(defn drain-fiber [fiber]
  "Resume fiber until dead, return final value."
  (def @result 0)
  (while (not= (fiber/status fiber) :dead) (assign result (fiber/resume fiber)))
  result)

# 3a: struct per iteration

(defn t3-yield-struct [n]
  (drain-fiber (fiber/new (fn []
                            (def before (arena/count))
                            (def @i 0)
                            (while (%lt i n)
                              {:x i :y (%add i 1)}
                              (yield i)
                              (assign i (%add i 1)))
                            (%sub (arena/count) before)) |:yield|)))

(let [d100 (t3-yield-struct 100)
      d10k (t3-yield-struct 10000)]
  (assert (or checked? (bounded? d100 d10k 10))
          (string "t3 yield-struct: d100=" d100 " d10k=" d10k)))

# 3b: string per iteration

(defn t3-yield-string [n]
  (drain-fiber (fiber/new (fn []
                            (def before (arena/count))
                            (def @i 0)
                            (while (%lt i n)
                              (string "iter-" i)
                              (yield i)
                              (assign i (%add i 1)))
                            (%sub (arena/count) before)) |:yield|)))

(let [d100 (t3-yield-string 100)
      d10k (t3-yield-string 10000)]
  (assert (or checked? (bounded? d100 d10k 20))
          (string "t3 yield-string: d100=" d100 " d10k=" d10k)))

# 3c: multiple allocations per iteration

(defn t3-yield-multi [n]
  (drain-fiber (fiber/new (fn []
                            (def before (arena/count))
                            (def @i 0)
                            (while (%lt i n)
                              {:x i}
                              (string "s" i)
                              (number->string i)
                              (yield i)
                              (assign i (%add i 1)))
                            (%sub (arena/count) before)) |:yield|)))

(let [d100 (t3-yield-multi 100)
      d10k (t3-yield-multi 10000)]
  (assert (or checked? (bounded? d100 d10k 30))
          (string "t3 yield-multi: d100=" d100 " d10k=" d10k)))

# ── Tier 0c: while loops with closures, fibers, concat, protect ──
# These patterns now pass escape analysis: closures are collected
# for rotation-safety analysis across lambda boundaries, and
# primitives no longer trigger false rejections for heap-returning args.

# Closure: let-bound lambda called inside while body
(defn t0c-closure-while [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (let [f (fn [] i)]
      (f))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t0c-closure-while 100)
      d10k (t0c-closure-while 10000)]
  (assert (or checked? (bounded? d100 d10k 10))
          (string "t0c closure-while: d100=" d100 " d10k=" d10k)))

# fiber/new + fiber/resume: primitives with heap-returning args
(defn t0c-fiber-while [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (let [f (fiber/new (fn [] i) 1)]
      (fiber/resume f))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t0c-fiber-while 100)
      d10k (t0c-fiber-while 10000)]
  (assert (or checked? (bounded? d100 d10k 10))
          (string "t0c fiber-while: d100=" d100 " d10k=" d10k)))

# concat with number->string: chain of primitives returning heap values
(defn t0c-concat-while [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (concat "x" (number->string i))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t0c-concat-while 100)
      d10k (t0c-concat-while 10000)]
  (assert (or checked? (bounded? d100 d10k 10))
          (string "t0c concat-while: d100=" d100 " d10k=" d10k)))

# protect: primitives creating closure + fiber internally
(defn t0c-protect-while [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (let [[ok v] (protect ((fn [] i)))]
      v)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t0c-protect-while 100)
      d10k (t0c-protect-while 10000)]
  (assert (or checked? (bounded? d100 d10k 10))
          (string "t0c protect-while: d100=" d100 " d10k=" d10k)))

# Same patterns in yielding while — also bounded now
(defn t0c-closure-yield [n]
  (drain-fiber (fiber/new (fn []
                            (def before (arena/count))
                            (def @i 0)
                            (while (%lt i n)
                              (let [f (fn [] i)]
                                (f))
                              (yield i)
                              (assign i (%add i 1)))
                            (%sub (arena/count) before)) |:yield|)))

(let [d100 (t0c-closure-yield 100)
      d10k (t0c-closure-yield 10000)]
  (assert (or checked? (bounded? d100 d10k 10))
          (string "t0c closure-yield: d100=" d100 " d10k=" d10k)))

(defn t0c-concat-yield [n]
  (drain-fiber (fiber/new (fn []
                            (def before (arena/count))
                            (def @i 0)
                            (while (%lt i n)
                              (concat "x" (number->string i))
                              (yield i)
                              (assign i (%add i 1)))
                            (%sub (arena/count) before)) |:yield|)))

(let [d100 (t0c-concat-yield 100)
      d10k (t0c-concat-yield 10000)]
  (assert (or checked? (bounded? d100 d10k 10))
          (string "t0c concat-yield: d100=" d100 " d10k=" d10k)))

# ── Tier 5: fiber lifecycle ─────────────────────────────────────
# Child fiber allocations live on a separate heap (arena/count measures
# the calling fiber's heap). Scope marks reclaim the FiberHandle slot
# on each iteration, triggering Drop on the child fiber and its heap.

# 5a: one-shot fiber — create, resume (completes), discard
(defn t5-one-shot [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (let [f (fiber/new (fn [] i) 1)]
      (fiber/resume f))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t5-one-shot 100)
      d2k (t5-one-shot 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t5 one-shot: d100=" d100 " d2k=" d2k)))

# 5b: child allocates a string, parent uses the result
(defn t5-alloc-return [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (let [f (fiber/new (fn [] (string "val-" i)) 1)
          result (fiber/resume f)]
      (assert (string? result)))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t5-alloc-return 100)
      d2k (t5-alloc-return 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t5 alloc-return: d100=" d100 " d2k=" d2k)))

# 5c: fiber inside a fiber body
(defn t5-nested [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (let [f (fiber/new (fn []
                         (let [g (fiber/new (fn [] i) 1)]
                           (fiber/resume g))) 1)]
      (fiber/resume f))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t5-nested 100)
      d2k (t5-nested 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t5 nested: d100=" d100 " d2k=" d2k)))

# 5d: create fiber, resume K times, discard
(defn t5-multi-resume [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (let [f (fiber/new (fn []
                         (yield 1)
                         (yield 2)
                         3) |:yield|)]
      (fiber/resume f)
      (fiber/resume f)
      (fiber/resume f))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t5-multi-resume 100)
      d2k (t5-multi-resume 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t5 multi-resume: d100=" d100 " d2k=" d2k)))

# 5e: protect in a loop (creates fiber internally)
(defn t5-protect [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (let [[ok v] (protect (+ 1 2))]
      v)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t5-protect 100)
      d2k (t5-protect 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t5 protect: d100=" d100 " d2k=" d2k)))

# ── Tier 6: collection HOFs in loops ──────────────────────────
# Higher-order stdlib functions that allocate intermediate structures.
# All should be bounded: results are discarded at scope exit.

# 6a: reduce
(defn t6-reduce [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (reduce + 0 [1 2 3])
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t6-reduce 100)
      d2k (t6-reduce 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t6 reduce: d100=" d100 " d2k=" d2k)))

# 6b: fold
(defn t6-fold [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (fold (fn [a x] (+ a x)) 0 [1 2 3])
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t6-fold 100)
      d2k (t6-fold 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t6 fold: d100=" d100 " d2k=" d2k)))

# 6c: zip
(defn t6-zip [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (zip [1 2] [3 4])
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t6-zip 100)
      d2k (t6-zip 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t6 zip: d100=" d100 " d2k=" d2k)))

# 6e: sort
(defn t6-sort [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (sort [3 1 2])
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t6-sort 100)
      d2k (t6-sort 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t6 sort: d100=" d100 " d2k=" d2k)))

# 6f: reverse
(defn t6-reverse [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (reverse [1 2 3])
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t6-reverse 100)
      d2k (t6-reverse 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t6 reverse: d100=" d100 " d2k=" d2k)))

# 6g: distinct
(defn t6-distinct [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (distinct [1 2 1 3])
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t6-distinct 100)
      d2k (t6-distinct 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t6 distinct: d100=" d100 " d2k=" d2k)))

# 6h: take and drop (operate on lists)
(defn t6-take-drop [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (take 2 (list 1 2 3))
    (drop 1 (list 1 2 3))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t6-take-drop 100)
      d2k (t6-take-drop 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t6 take-drop: d100=" d100 " d2k=" d2k)))

# 6i: group-by
(defn t6-group-by [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (group-by odd? [1 2 3 4])
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t6-group-by 100)
      d2k (t6-group-by 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t6 group-by: d100=" d100 " d2k=" d2k)))

# 6j: frequencies
(defn t6-frequencies [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (frequencies [1 2 1 3])
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t6-frequencies 100)
      d2k (t6-frequencies 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t6 frequencies: d100=" d100 " d2k=" d2k)))

# ── Tier 7: collection conversions and slicing ────────────────

# 7a: ->array
(defn t7-to-array [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (->array (list 1 2 3))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t7-to-array 100)
      d2k (t7-to-array 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t7 ->array: d100=" d100 " d2k=" d2k)))

# 7b: ->list
(defn t7-to-list [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (->list [1 2 3])
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t7-to-list 100)
      d2k (t7-to-list 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t7 ->list: d100=" d100 " d2k=" d2k)))

# 7c: freeze mutable array
(defn t7-freeze [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (freeze @[1 2 3])
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t7-freeze 100)
      d2k (t7-freeze 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t7 freeze: d100=" d100 " d2k=" d2k)))

# 7d: slice
(defn t7-slice [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (slice [1 2 3 4] 1 3)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t7-slice 100)
      d2k (t7-slice 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t7 slice: d100=" d100 " d2k=" d2k)))

# 7e: keys and values
(defn t7-keys-values [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (keys {:a 1 :b 2})
    (values {:a 1 :b 2})
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t7-keys-values 100)
      d2k (t7-keys-values 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t7 keys-values: d100=" d100 " d2k=" d2k)))

# 7f: merge
(defn t7-merge [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (merge {:a 1} {:b 2})
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t7-merge 100)
      d2k (t7-merge 2000)]
  (assert (or checked? (bounded? d100 d2k 30))
          (string "t7 merge: d100=" d100 " d2k=" d2k)))

# ── Tier 8: string operations in loops ────────────────────────

# 8a: string interpolation
(defn t8-string-interp [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (string "x=" i " y=" (%add i 1))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t8-string-interp 100)
      d2k (t8-string-interp 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t8 string-interp: d100=" d100 " d2k=" d2k)))

# 8b: concat chain
(defn t8-concat [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (concat "a" "b" "c")
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t8-concat 100)
      d2k (t8-concat 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t8 concat: d100=" d100 " d2k=" d2k)))

# 8c: string/split
(defn t8-split [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (string/split "a,b,c" ",")
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t8-split 100)
      d2k (t8-split 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t8 split: d100=" d100 " d2k=" d2k)))

# 8d: string/join
(defn t8-join [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (string/join ["a" "b" "c"] ",")
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t8-join 100)
      d2k (t8-join 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t8 join: d100=" d100 " d2k=" d2k)))

# 8e: string/trim
(defn t8-trim [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (string/trim "  x  ")
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t8-trim 100)
      d2k (t8-trim 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t8 trim: d100=" d100 " d2k=" d2k)))

# 8f: string/replace
(defn t8-replace [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (string/replace "hello" "l" "r")
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t8-replace 100)
      d2k (t8-replace 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t8 replace: d100=" d100 " d2k=" d2k)))

# 8g: number->string
(defn t8-num-to-str [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (number->string i)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t8-num-to-str 100)
      d2k (t8-num-to-str 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t8 num->str: d100=" d100 " d2k=" d2k)))

# 8h: read
(defn t8-read [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (read "42")
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t8-read 100)
      d2k (t8-read 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t8 read: d100=" d100 " d2k=" d2k)))

# ── Tier 9: struct patterns in loops ──────────────────────────

# 9a: struct literal discarded
(defn t9-struct-lit [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    {:x i :y (%add i 1)}
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t9-struct-lit 100)
      d2k (t9-struct-lit 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t9 struct-lit: d100=" d100 " d2k=" d2k)))

# 9b: struct field access (no alloc escape)
(defn t9-struct-get [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (let [s {:x 1}]
      s:x)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t9-struct-get 100)
      d2k (t9-struct-get 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t9 struct-get: d100=" d100 " d2k=" d2k)))

# 9c: mutable struct created and discarded (put with immediate value)
(defn t9-struct-put [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (let [s @{:x 0}]
      (put s :x i))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t9-struct-put 100)
      d2k (t9-struct-put 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t9 struct-put: d100=" d100 " d2k=" d2k)))

# 9e: struct match — pattern matching on struct, no escape
(defn t9-struct-match [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (match {:type :a :v i}
      {:type :a :v v} v
      _ 0)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t9-struct-match 100)
      d2k (t9-struct-match 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t9 struct-match: d100=" d100 " d2k=" d2k)))

# ── Tier 10: combined patterns (realistic code) ──────────────

(defn helper-f [x]
  (string "v" x))
(defn helper-g [x]
  {:val x})
(defn helper-h [x]
  (+ x 1))

# 10a: nested function calls, each allocating
(defn t10-call-chain [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (helper-f (helper-g (helper-h i)))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t10-call-chain 100)
      d2k (t10-call-chain 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t10 call-chain: d100=" d100 " d2k=" d2k)))

# 10b: let chain
(defn t10-let-chain [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (let [a (helper-h i)]
      (let [b (helper-g a)]
        b))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t10-let-chain 100)
      d2k (t10-let-chain 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t10 let-chain: d100=" d100 " d2k=" d2k)))

# 10c: each over array with string alloc per element
(defn t10-each-array [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (each x in [1 2 3]
      (string "v" x))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t10-each-array 100)
      d2k (t10-each-array 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t10 each-array: d100=" d100 " d2k=" d2k)))

# 10e: push into accumulator
# With refcounting, the pushed values survive (incref'd by push),
# but temporaries (closure, intermediate arrays) are freed at scope exit.
# arena/count stays bounded because scope marks reclaim dead objects.
(defn t10-push-accum [n]
  (def before (arena/count))
  (def @acc @[])
  (def @i 0)
  (while (%lt i n)
    (push acc (map (fn [x] (%add x 1)) [1 2 3]))
    (assign i (%add i 1)))  # Verify correctness: all elements must be [2 3 4]
  (assert (= (get acc 0) [2 3 4]) "push-accum: first element")
  (assert (= (get acc (%sub n 1)) [2 3 4]) "push-accum: last element")
  (%sub (arena/count) before))

(let [d100 (t10-push-accum 100)
      d2k (t10-push-accum 2000)]
  (assert (or checked? (not (bounded? d100 d2k 30)))
          (string "t10 push-accum: FIXED? remove regression marker. d100=" d100
                  " d2k=" d2k)))

# 10f: format-string in a loop (println is async I/O; test formatting only)
(defn t10-format [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (string "iter " i " of " n)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t10-format 100)
      d2k (t10-format 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t10 format: d100=" d100 " d2k=" d2k)))

# 10g: pipeline — split → map → filter → join
(defn t10-pipeline [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (string/join (filter (fn [x] (not= x ""))
                         (map string/trim (string/split "a , b , c" ","))) ",")
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t10-pipeline 100)
      d2k (t10-pipeline 2000)]
  (assert (or checked? (bounded? d100 d2k 10))
          (string "t10 pipeline: d100=" d100 " d2k=" d2k)))

# ── Known leaks: inherent ────────────────────────────────────────
# These genuinely escape heap values to outer bindings or collections.
# The scope cannot free them because the outer reference survives.
# Fixing requires drop-on-overwrite or reference counting.

# Heap struct assigned to outer mutable binding
# With the bump arena, release() reclaims memory by position rewind
# regardless of escape analysis. The test verifies allocations happen.
(defn leak-struct-outer [n]
  (def before (arena/count))
  (def @last nil)
  (def @i 0)
  (while (%lt i n)
    (assign last {:x i})
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (leak-struct-outer 100)
      d1k (leak-struct-outer 1000)]
  (assert (<= d1k (+ d100 2)) (string "struct-outer: d100=" d100 " d1k=" d1k)))

# Heap string assigned to outer mutable binding (concat accumulation)
(defn leak-string-outer [n]
  (def before (arena/count))
  (def @s "")
  (def @i 0)
  (while (%lt i n)
    (assign s (concat s "x"))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (leak-string-outer 100)
      d1k (leak-string-outer 1000)]
  (assert (<= d1k (+ d100 2)) (string "string-outer: d100=" d100 " d1k=" d1k)))

# Heap array assigned to outer mutable binding (append accumulation)
(defn leak-append-outer [n]
  (def before (arena/count))
  (def @acc [])
  (def @i 0)
  (while (%lt i n)
    (assign acc (append acc [i]))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (leak-append-outer 100)
      d1k (leak-append-outer 1000)]
  (assert (<= d1k (+ d100 2)) (string "append-outer: d100=" d100 " d1k=" d1k)))

# push stores heap struct into outer mutable array
# With refcounting, pushed values are incref'd and survive scope exit.
# arena/count stays bounded because scope marks reclaim dead temporaries.
(defn leak-push-outer [n]
  (def before (arena/count))
  (def @acc @[])
  (def @i 0)
  (while (%lt i n)
    (push acc {:x i})
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (leak-push-outer 100)
      d10k (leak-push-outer 10000)]
  (assert (or checked? (bounded? d100 d10k 30))
          (string "push-outer: d100=" d100 " d10k=" d10k)))

# put stores heap string into outer mutable struct
# With refcounting: old value decref'd on overwrite → freed at scope exit.
(defn leak-put-outer [n]
  (def before (arena/count))
  (def @s @{:x 0})
  (def @i 0)
  (while (%lt i n)
    (put s :x (string "v" i))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (leak-put-outer 100)
      d10k (leak-put-outer 10000)]
  (assert (or checked? (bounded? d100 d10k 30))
          (string "put-outer: d100=" d100 " d10k=" d10k)))

# ── Known leaks: fixable (escape analysis limitations) ──────────
# These don't genuinely escape heap values but are rejected by
# escape analysis conservatism. When fixed, flip to bounded?.

# each over lists: the each macro desugars to coroutines. QW2
# recognizes internal stdlib calls as non-escaping, enabling FlipSwap
# on the outer while loop. The coroutine is fully drained within
# each iteration, so FlipSwap at the back-edge is safe.
(defn leak-each-list [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (each x in (list 1 2 3)
      {:val x})
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (leak-each-list 100)
      d1k (leak-each-list 1000)]
  (assert (or checked? (bounded? d100 d1k 10))
          (string "each-list: d100=" d100 " d1k=" d1k)))

# map in while: map is a stdlib HOF — QW2 recognizes non-escaping
# stdlib functions, so FlipSwap is now eligible.
(defn leak-map-while [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (map (fn [x] (%add x 1)) [1 2 3])
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (leak-map-while 100)
      d1k (leak-map-while 1000)]
  (assert (or checked? (bounded? d100 d1k 10))
          (string "map-while: d100=" d100 " d1k=" d1k)))

# filter in while: same as map — QW2 recognizes filter.
(defn leak-filter-while [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (filter (fn [x] (%gt x 1)) [1 2 3])
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (leak-filter-while 100)
      d1k (leak-filter-while 1000)]
  (assert (or checked? (bounded? d100 d1k 10))
          (string "filter-while: d100=" d100 " d1k=" d1k)))

# nested closure: (fn [] (fn [] i)) — QW3 traces through calls to
# rotation-safe let-bound lambdas to prove the outer call is safe.
(defn leak-nested-closure [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (let [f (fn [] (fn [] i))]
      ((f)))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (leak-nested-closure 100)
      d1k (leak-nested-closure 1000)]
  (assert (or checked? (bounded? d100 d1k 10))
          (string "nested-closure: d100=" d100 " d1k=" d1k)))

# ── Tier 4: correctness under rotation ───────────────────────────
# Rotation must not corrupt live values. Returned heap values and
# yielded heap values must survive.

# Tail-call return value survives rotation.
(defn t4-return [n]
  (if (= n 0)
    (string "result-" n)
    (begin
      {:x n}
      (t4-return (%sub n 1)))))

(assert (= (t4-return 10000) "result-0")
        (string "t4 return: " (t4-return 10000)))

# Yielded heap values survive across resume boundaries.
(let* [fiber (fiber/new (fn []
                          (def @i 0)
                          (while (%lt i 100)
                            (yield (string "val-" i))
                            (assign i (%add i 1)))) |:yield|)
       first-val (fiber/resume fiber)
       second-val (fiber/resume fiber)]
  (assert (= first-val "val-0") (string "t4 yield survives: " first-val))
  (assert (= second-val "val-1") (string "t4 yield survives: " second-val)))

# Accumulator threaded through tail calls survives rotation.
(defn t4-accum [n acc]
  (if (= n 0) acc (t4-accum (%sub n 1) (%add acc n))))

(assert (= (t4-accum 10000 0) 50005000)
        (string "t4 accumulator: " (t4-accum 10000 0)))

# Yielded heap values survive per-iteration scope release at scale.
(let* [fiber (fiber/new (fn []
                          (def @i 0)
                          (while (%lt i 1000)
                            (yield (string "val-" i))
                            (assign i (%add i 1)))) |:yield|)
       vals (do
              (def @acc [])
              (while (not= (fiber/status fiber) :dead)
                (assign acc (append acc [(fiber/resume fiber)])))
              acc)]
  (assert (= (get vals 0) "val-0")
          (string "t4 yield-at-scale first: " (get vals 0)))
  (assert (= (get vals 999) "val-999")
          (string "t4 yield-at-scale last: " (get vals 999))))

# ── Tier 11: refcount mutation reclamation ───────────────────
# These test that overwritten heap values in mutable collections
# and mutable bindings are reclaimed via deferred reference counting.
# Before refcounting, these leak linearly. After, they are bounded.
#
# Type-aware signal narrowing strips SIG_ERROR from calls with
# provably typed arguments, unblocking refcounted rotation.

# 11a: put overwrites heap string in mutable struct — old value freed
(defn t11-put-overwrite [n]
  (def before (arena/count))
  (def @s @{:key 0})
  (def @i 0)
  (while (%lt i n)
    (put s :key (string "v" i))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t11-put-overwrite 100)
      d10k (t11-put-overwrite 10000)]
  (assert (or checked? (bounded? d100 d10k 30))
          (string "t11 put-overwrite: d100=" d100 " d10k=" d10k)))

# 11b: put overwrites heap struct in mutable struct
(defn t11-put-struct [n]
  (def before (arena/count))
  (def @s @{:data nil})
  (def @i 0)
  (while (%lt i n)
    (put s :data {:iter i})
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t11-put-struct 100)
      d10k (t11-put-struct 10000)]
  (assert (or checked? (bounded? d100 d10k 30))
          (string "t11 put-struct: d100=" d100 " d10k=" d10k)))

# 11c: set overwrites heap value in mutable array
(defn t11-set-array [n]
  (def before (arena/count))
  (def @arr @[(string "init")])
  (def @i 0)
  (while (%lt i n)
    (put arr 0 (string "v" i))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t11-set-array 100)
      d10k (t11-set-array 10000)]
  (assert (or checked? (bounded? d100 d10k 30))
          (string "t11 set-array: d100=" d100 " d10k=" d10k)))

# 11d: multiple puts per iteration (roster pattern)
(defn t11-roster [n]
  (def before (arena/count))
  (def @trading @{:pnl 0 :trades 0 :label ""})
  (def @i 0)
  (while (%lt i n)
    (put trading :pnl (%add i 100))
    (put trading :trades (%add i 1))
    (put trading :label (string "trade-" i))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t11-roster 100)
      d10k (t11-roster 10000)]
  (assert (or checked? (bounded? d100 d10k 30))
          (string "t11 roster: d100=" d100 " d10k=" d10k)))

# 11e: mutable binding reassignment with heap values
(defn t11-binding-reassign [n]
  (def before (arena/count))
  (def @v (string "init"))
  (def @i 0)
  (while (%lt i n)
    (assign v (string "val-" i))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t11-binding-reassign 100)
      d10k (t11-binding-reassign 10000)]
  (assert (or checked? (bounded? d100 d10k 30))
          (string "t11 binding-reassign: d100=" d100 " d10k=" d10k)))

# ── Tier 12: user functions in while loops ──────────────────────
# Calling a user-defined function that allocates internally should
# NOT prevent scope reclamation in the caller's while loop. The callee
# runs and returns before rotation; its internal allocations cannot
# cause the caller's scope-allocated values to dangle.

# 12a: user function returning a struct
(defn make-struct [i]
  {:iter i :val (%add i 1)})

(defn t12-user-struct [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (make-struct i)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t12-user-struct 100)
      d10k (t12-user-struct 10000)]
  (assert (or checked? (bounded? d100 d10k 30))
          (string "t12 user-struct: d100=" d100 " d10k=" d10k)))

# 12b: user function returning a string
(defn make-label [i]
  (string "item-" i))

(defn t12-user-string [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (make-label i)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t12-user-string 100)
      d10k (t12-user-string 10000)]
  (assert (or checked? (bounded? d100 d10k 30))
          (string "t12 user-string: d100=" d100 " d10k=" d10k)))

# 12c: chained user functions (user fn calls another user fn)
(defn process [i]
  (make-struct (%add i 10)))

(defn t12-chain [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (process i)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t12-chain 100)
      d10k (t12-chain 10000)]
  (assert (or checked? (bounded? d100 d10k 30))
          (string "t12 chain: d100=" d100 " d10k=" d10k)))

# 12d: user function wrapping map (stdlib HOF)
(defn transform [xs]
  (map (fn [x] (%add x 1)) xs))

(defn t12-wrap-map [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (transform [1 2 3])
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t12-wrap-map 100)
      d10k (t12-wrap-map 10000)]
  (assert (or checked? (bounded? d100 d10k 30))
          (string "t12 wrap-map: d100=" d100 " d10k=" d10k)))

# ── Tier 13: value-flow propagation ───────────────────────────
# Closures obtained through non-Lambda value paths (factory calls,
# aliases, conditional construction) should be recognized as
# rotation-safe / param-safe so the caller's while loop can reclaim.

# 13a: factory function returning a closure
(defn make-proc []
  (fn [i] {:x i}))

(defn t13-factory [n]
  (def proc (make-proc))
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (proc i)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t13-factory 100)
      d10k (t13-factory 10000)]
  (assert (or checked? (bounded? d100 d10k 30))
          (string "t13 factory: d100=" d100 " d10k=" d10k)))

# 13b: conditional factory — both branches return closures
(defn make-thing [mode]
  (if (= mode :fast) (fn [x] {:fast x}) (fn [x] {:slow x})))

(defn t13-cond-factory [n]
  (def proc (make-thing :fast))
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (proc i)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t13-cond-factory 100)
      d10k (t13-cond-factory 10000)]
  (assert (or checked? (bounded? d100 d10k 30))
          (string "t13 cond-factory: d100=" d100 " d10k=" d10k)))

# 13c: alias to a known-safe function
(defn t13-alias [n]
  (def f make-struct)
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (f i)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t13-alias 100)
      d10k (t13-alias 10000)]
  (assert (or checked? (bounded? d100 d10k 30))
          (string "t13 alias: d100=" d100 " d10k=" d10k)))

# 13d: nested factory — factory calls another factory
(defn make-inner []
  (fn [x] {:x x}))
(defn make-outer []
  (make-inner))

(defn t13-nested-factory [n]
  (def proc (make-outer))
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (proc i)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t13-nested-factory 100)
      d10k (t13-nested-factory 10000)]
  (assert (or checked? (bounded? d100 d10k 30))
          (string "t13 nested-factory: d100=" d100 " d10k=" d10k)))

# 13e: let-bound alias
(defn t13-let-alias [n]
  (let [f make-struct]
    (def before (arena/count))
    (def @i 0)
    (while (%lt i n)
      (f i)
      (assign i (%add i 1)))
    (%sub (arena/count) before)))

(let [d100 (t13-let-alias 100)
      d10k (t13-let-alias 10000)]
  (assert (or checked? (bounded? d100 d10k 30))
          (string "t13 let-alias: d100=" d100 " d10k=" d10k)))

# 13f: struct-field callee — module pattern
(defn make-module []
  (defn mod-make [i]
    {:x i})
  (defn mod-label [i]
    (string "item-" i))
  {:make mod-make :label mod-label})

(def the-mod (make-module))

(defn t13-struct-field [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (the-mod:make i)
    (the-mod:label i)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (t13-struct-field 100)
      d10k (t13-struct-field 10000)]
  (assert (or checked? (bounded? d100 d10k 30))
          (string "t13 struct-field: d100=" d100 " d10k=" d10k)))
