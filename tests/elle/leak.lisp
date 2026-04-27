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

# ── Tier 0c: while loops with closures, fibers, concat, protect ──
# These patterns now pass escape analysis: closures are collected
# for rotation-safety analysis across lambda boundaries, and
# primitives no longer trigger false rejections for heap-returning args.

# Closure: let-bound lambda called inside while body
(defn t0c-closure-while [n]
  (def before (arena/count))
  (def @i 0)
  (while (< i n)
    (let [f (fn [] i)] (f))
    (assign i (+ i 1)))
  (- (arena/count) before))

(let [d100 (t0c-closure-while 100) d10k (t0c-closure-while 10000)]
  (assert (bounded? d100 d10k 10)
    (string "t0c closure-while: d100=" d100 " d10k=" d10k)))

# fiber/new + fiber/resume: primitives with heap-returning args
(defn t0c-fiber-while [n]
  (def before (arena/count))
  (def @i 0)
  (while (< i n)
    (let [f (fiber/new (fn [] i) 1)] (fiber/resume f))
    (assign i (+ i 1)))
  (- (arena/count) before))

(let [d100 (t0c-fiber-while 100) d10k (t0c-fiber-while 10000)]
  (assert (bounded? d100 d10k 10)
    (string "t0c fiber-while: d100=" d100 " d10k=" d10k)))

# concat with number->string: chain of primitives returning heap values
(defn t0c-concat-while [n]
  (def before (arena/count))
  (def @i 0)
  (while (< i n)
    (concat "x" (number->string i))
    (assign i (+ i 1)))
  (- (arena/count) before))

(let [d100 (t0c-concat-while 100) d10k (t0c-concat-while 10000)]
  (assert (bounded? d100 d10k 10)
    (string "t0c concat-while: d100=" d100 " d10k=" d10k)))

# protect: primitives creating closure + fiber internally
(defn t0c-protect-while [n]
  (def before (arena/count))
  (def @i 0)
  (while (< i n)
    (let [[ok v] (protect ((fn [] i)))] v)
    (assign i (+ i 1)))
  (- (arena/count) before))

(let [d100 (t0c-protect-while 100) d10k (t0c-protect-while 10000)]
  (assert (bounded? d100 d10k 10)
    (string "t0c protect-while: d100=" d100 " d10k=" d10k)))

# Same patterns in yielding while — also bounded now
(defn t0c-closure-yield [n]
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

(let [d100 (t0c-closure-yield 100) d10k (t0c-closure-yield 10000)]
  (assert (bounded? d100 d10k 10)
    (string "t0c closure-yield: d100=" d100 " d10k=" d10k)))

(defn t0c-concat-yield [n]
  (drain-fiber
    (fiber/new
      (fn []
        (def before (arena/count))
        (def @i 0)
        (while (< i n)
          (concat "x" (number->string i))
          (yield i)
          (assign i (+ i 1)))
        (- (arena/count) before))
      |:yield|)))

(let [d100 (t0c-concat-yield 100) d10k (t0c-concat-yield 10000)]
  (assert (bounded? d100 d10k 10)
    (string "t0c concat-yield: d100=" d100 " d10k=" d10k)))

# ── Known leak: struct assigned to outer mutable binding ─────────
# heap-allocated assign to an outer mutable binding is genuinely
# dangerous (the scope would free it while the outer binding still
# holds a reference). This remains linear.

(defn linear? [d100 d1000]
  "True if growth is roughly linear (d1000 ≥ 5x d100)."
  (and (>= d100 50) (>= d1000 (* d100 5))))

(defn leak-struct-outer [n]
  (def before (arena/count))
  (def @last nil)
  (def @i 0)
  (while (< i n)
    (assign last {:x i})
    (assign i (+ i 1)))
  (- (arena/count) before))

(let [d100 (leak-struct-outer 100) d1k (leak-struct-outer 1000)]
  (assert (linear? d100 d1k)
    (string "struct-outer: d100=" d100 " d1k=" d1k " — expected linear leak")))

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

# Yielded heap values survive per-iteration scope release at scale.
(let* [fiber (fiber/new
               (fn []
                 (def @i 0)
                 (while (< i 1000)
                   (yield (string "val-" i))
                   (assign i (+ i 1))))
               |:yield|)
       vals (do
              (def @acc [])
              (while (not= (fiber/status fiber) :dead)
                (assign acc (append acc [(fiber/resume fiber)])))
              acc)]
  (assert (= (get vals 0) "val-0")
    (string "t4 yield-at-scale first: " (get vals 0)))
  (assert (= (get vals 999) "val-999")
    (string "t4 yield-at-scale last: " (get vals 999))))
