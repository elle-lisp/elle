(elle/epoch 9)
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

(defn bounded? [d100 d10k limit]
  "True if both deltas are under limit and 10000 is not 100x 100."
  (and (< d100 limit)
       (< d10k limit)
       (or (= d100 0)
           (< d10k (* d100 10)))))

# ── Tier 0: scope reclamation in while loops ─────────────────────
# Let-bound structs inside a while body are reclaimed by region-exit.

(defn t0-let-struct [n]
  (def before (arena/count))
  (def @i 0)
  (while (< i n)
    (let [x {:iter i}] x)
    (assign i (+ i 1)))
  (- (arena/count) before))

(let [d100 (t0-let-struct 100) d10k (t0-let-struct 10000)]
  (assert (bounded? d100 d10k 10)
    (string "t0 let-struct: d100=" d100 " d10k=" d10k)))

# Discarded struct (no let binding) — scope still reclaims.

(defn t0-discard-struct [n]
  (def before (arena/count))
  (def @i 0)
  (while (< i n)
    {:x i :y (+ i 1)}
    (assign i (+ i 1)))
  (- (arena/count) before))

(let [d100 (t0-discard-struct 100) d10k (t0-discard-struct 10000)]
  (assert (bounded? d100 d10k 10)
    (string "t0 discard-struct: d100=" d100 " d10k=" d10k)))

# String allocation in while loop — scope reclaims.

(defn t0-string [n]
  (def before (arena/count))
  (def @i 0)
  (while (< i n)
    (string "iter-" i)
    (assign i (+ i 1)))
  (- (arena/count) before))

(let [d100 (t0-string 100) d10k (t0-string 10000)]
  (assert (bounded? d100 d10k 10)
    (string "t0 string: d100=" d100 " d10k=" d10k)))

# ── Tier 1: nested while loops ───────────────────────────────────
# Inner and outer loops both allocate; scoping must handle both.

(defn t1-nested [outer inner]
  (def before (arena/count))
  (def @i 0)
  (while (< i outer)
    (def @j 0)
    (while (< j inner)
      {:x i :y j}
      (assign j (+ j 1)))
    (assign i (+ i 1)))
  (- (arena/count) before))

(let [d-small (t1-nested 10 10) d-big (t1-nested 100 100)]
  (assert (< d-small 20)
    (string "t1 nested small: " d-small))
  (assert (< d-big 20)
    (string "t1 nested big: " d-big)))

# ── Tier 2: tail-call rotation ───────────────────────────────────
# Tail-recursive loop allocating a struct per iteration. Trampoline
# rotation + flip keep alloc_count bounded.

(defn t2-struct [n]
  (if (= n 0) (arena/count)
    (begin {:x n} (t2-struct (- n 1)))))

(let* [b1 (arena/count) a1 (t2-struct 100) d100 (- a1 b1)
       b2 (arena/count) a2 (t2-struct 10000) d10k (- a2 b2)]
  (assert (bounded? d100 d10k 10)
    (string "t2 struct: d100=" d100 " d10k=" d10k)))

# Tail-recursive string allocation.

(defn t2-string [n]
  (if (= n 0) (arena/count)
    (begin (string "iter-" n) (t2-string (- n 1)))))

(let* [b1 (arena/count) a1 (t2-string 100) d100 (- a1 b1)
       b2 (arena/count) a2 (t2-string 10000) d10k (- a2 b2)]
  (assert (bounded? d100 d10k 10)
    (string "t2 string: d100=" d100 " d10k=" d10k)))

# Mutual tail recursion with struct allocation.

(defn t2-even [n]
  (if (= n 0) (arena/count)
    (begin {:parity :even :n n} (t2-odd (- n 1)))))

(defn t2-odd [n]
  (if (= n 0) (arena/count)
    (begin {:parity :odd :n n} (t2-even (- n 1)))))

(let* [b1 (arena/count) a1 (t2-even 100) d100 (- a1 b1)
       b2 (arena/count) a2 (t2-even 10000) d10k (- a2 b2)]
  (assert (bounded? d100 d10k 10)
    (string "t2 mutual: d100=" d100 " d10k=" d10k)))

# ── Tier 3: yielding while loops (flip is essential) ─────────────
# A fiber yields mid-iteration, so scope regions cannot reclaim —
# the fiber suspends before region-exit. Without flip, allocations
# accumulate linearly. With flip, FlipSwap at the back-edge rotates
# pools each iteration.

(defn drain-fiber [fiber]
  "Resume fiber until dead, return final value."
  (def @result 0)
  (while (not= (fiber/status fiber) :dead)
    (assign result (fiber/resume fiber)))
  result)

# 3a: struct per iteration

(defn t3-yield-struct [n]
  (drain-fiber
    (fiber/new
      (fn []
        (def before (arena/count))
        (def @i 0)
        (while (< i n)
          {:x i :y (+ i 1)}
          (yield i)
          (assign i (+ i 1)))
        (- (arena/count) before))
      |:yield|)))

(let [d100 (t3-yield-struct 100) d10k (t3-yield-struct 10000)]
  (assert (bounded? d100 d10k 10)
    (string "t3 yield-struct: d100=" d100 " d10k=" d10k)))

# 3b: string per iteration

(defn t3-yield-string [n]
  (drain-fiber
    (fiber/new
      (fn []
        (def before (arena/count))
        (def @i 0)
        (while (< i n)
          (string "iter-" i)
          (yield i)
          (assign i (+ i 1)))
        (- (arena/count) before))
      |:yield|)))

(let [d100 (t3-yield-string 100) d10k (t3-yield-string 10000)]
  (assert (bounded? d100 d10k 20)
    (string "t3 yield-string: d100=" d100 " d10k=" d10k)))

# 3c: multiple allocations per iteration

(defn t3-yield-multi [n]
  (drain-fiber
    (fiber/new
      (fn []
        (def before (arena/count))
        (def @i 0)
        (while (< i n)
          {:x i}
          (string "s" i)
          (number->string i)
          (yield i)
          (assign i (+ i 1)))
        (- (arena/count) before))
      |:yield|)))

(let [d100 (t3-yield-multi 100) d10k (t3-yield-multi 10000)]
  (assert (bounded? d100 d10k 30)
    (string "t3 yield-multi: d100=" d100 " d10k=" d10k)))

# ── Tier 3d: closure leak in while loops (known defect) ──────────
# Closures are dtor-bearing objects (Rc<ClosureTemplate>). rotate_pools
# keeps them in the main pool to avoid freeing Rc inners while still
# referenced, and escape analysis rejects while loops containing
# closure allocations. This causes linear accumulation.
#
# These assertions document the current leak. When the defect is
# fixed, they will fail — update them to assert bounded.

(defn t3-closure-while [n]
  (def before (arena/count))
  (def @i 0)
  (while (< i n)
    (let [f (fn [] i)] (f))
    (assign i (+ i 1)))
  (- (arena/count) before))

(let [d100 (t3-closure-while 100) d10k (t3-closure-while 10000)]
  (assert (= d100 100)
    (string "t3d closure-while: expected linear leak, d100=" d100))
  (assert (= d10k 10000)
    (string "t3d closure-while: expected linear leak, d10k=" d10k)))

(defn t3-closure-yield [n]
  (drain-fiber
    (fiber/new
      (fn []
        (def before (arena/count))
        (def @i 0)
        (while (< i n)
          (let [f (fn [] i)] (f))
          (yield i)
          (assign i (+ i 1)))
        (- (arena/count) before))
      |:yield|)))

(let [d100 (t3-closure-yield 100) d10k (t3-closure-yield 1000)]
  (assert (= d100 100)
    (string "t3d closure-yield: expected linear leak, d100=" d100))
  (assert (= d10k 1000)
    (string "t3d closure-yield: expected linear leak, d10k=" d10k)))

# ── Tier 4: correctness under rotation ───────────────────────────
# Rotation must not corrupt live values. Returned heap values and
# yielded heap values must survive.

# Tail-call return value survives rotation.
(defn t4-return [n]
  (if (= n 0) (string "result-" n)
    (begin {:x n} (t4-return (- n 1)))))

(assert (= (t4-return 10000) "result-0")
  (string "t4 return: " (t4-return 10000)))

# Yielded heap values survive across resume boundaries.
(let* [fiber (fiber/new
               (fn []
                 (def @i 0)
                 (while (< i 100)
                   (yield (string "val-" i))
                   (assign i (+ i 1))))
               |:yield|)
       first-val (fiber/resume fiber)
       second-val (fiber/resume fiber)]
  (assert (= first-val "val-0")
    (string "t4 yield survives: " first-val))
  (assert (= second-val "val-1")
    (string "t4 yield survives: " second-val)))

# Accumulator threaded through tail calls survives rotation.
(defn t4-accum [n acc]
  (if (= n 0) acc
    (t4-accum (- n 1) (+ acc n))))

(assert (= (t4-accum 10000 0) 50005000)
  (string "t4 accumulator: " (t4-accum 10000 0)))
