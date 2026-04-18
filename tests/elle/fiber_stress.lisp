(elle/epoch 7)
## Fiber control-passing stress tests
##
## Tests sustained resume loops and deep nesting to verify
## signal propagation works correctly over many iterations.

# ============================================================================
# Sustained coroutine resume (25 iterations, direct yield)
# ============================================================================

(begin
  (var co (make-coroutine (fn []
    (let [i 0]
      (while (< i 25)
        (yield i)
        (assign i (+ i 1)))
      i))))
  (let [i 0]
    (while (< i 25)
      (assert (= (coro/resume co) i)
              (string "sustained direct yield: iteration " i))
      (assign i (+ i 1))))
  (assert (= (coro/resume co) 25) "sustained direct yield: final return"))

# ============================================================================
# Sustained coroutine with resume values
# ============================================================================

(begin
  (var co (make-coroutine (fn []
    (let [acc 0]
      (let [i 0]
        (while (< i 20)
          (let [v (yield acc)]
            (assign acc (+ acc v)))
          (assign i (+ i 1))))
      acc))))
  # First resume starts the coroutine, yields acc=0
  (assert (= (coro/resume co) 0) "resume values: initial acc")
  (let [expected 0
        i 1]
    (while (<= i 20)
      (let [result (coro/resume co i)]
        (assign expected (+ expected i))
        (if (<= i 19)
          (assert (= result expected)
                  (string "resume values: iteration " i " acc=" result " expected=" expected))
          (assert (= result expected)
                  (string "resume values: final acc=" result " expected=" expected))))
      (assign i (+ i 1)))))

# ============================================================================
# Sustained fiber/resume (raw fibers, not coroutines)
# ============================================================================

(begin
  (let [f (fiber/new (fn []
              (let [i 0]
                (while (< i 20)
                  (yield i)
                  (assign i (+ i 1)))
                :done))
            2)]
    (let [i 0]
      (while (< i 20)
        (assert (= (fiber/resume f) i)
                (string "sustained fiber emit: iteration " i))
        (assign i (+ i 1))))
    (assert (= (fiber/resume f) :done) "sustained fiber emit: final")))

# ============================================================================
# Sustained yield-through-call
# ============================================================================

(begin
  (defn yielder (x) (yield x))
  (var co (make-coroutine (fn []
    (let [i 0]
      (while (< i 20)
        (yielder i)
        (assign i (+ i 1)))
      :done))))
  (let [i 0]
    (while (< i 20)
      (assert (= (coro/resume co) i)
              (string "yield-through-call: iteration " i))
      (assign i (+ i 1))))
  (assert (= (coro/resume co) :done) "yield-through-call: final"))

# ============================================================================
# Nested fiber with sustained inner resumes
# ============================================================================

(begin
  (let [inner (fiber/new (fn []
                  (let [i 0]
                    (while (< i 15)
                      (yield i)
                      (assign i (+ i 1)))
                    :inner-done))
                2)]
    (let [outer (fiber/new (fn []
                    (let [i 0
                          results @[]]
                      (while (< i 15)
                        (push results (fiber/resume inner))
                        (assign i (+ i 1)))
                      (push results (fiber/resume inner))
                      results))
                  0)]
      (let [result (fiber/resume outer)]
        (assert (= (length result) 16) "nested sustained: got 16 results")
        (assert (= (get result 0) 0) "nested sustained: first is 0")
        (assert (= (get result 14) 14) "nested sustained: 15th is 14")
        (assert (= (get result 15) :inner-done) "nested sustained: last is :inner-done")))))

# ============================================================================
# Deep call chain yield-through-call (3 levels)
# ============================================================================

(begin
  (defn deep-yield (x) (yield x))
  (defn mid (x) (deep-yield x))
  (defn top (x) (mid x))
  (var co (make-coroutine (fn []
    (let [i 0]
      (while (< i 20)
        (top (* i 10))
        (assign i (+ i 1)))
      :done))))
  (let [i 0]
    (while (< i 20)
      (let [v (coro/resume co)]
        (assert (= v (* i 10))
                (string "deep chain: yield " i " expected " (* i 10) " got " v)))
      (assign i (+ i 1))))
  (assert (= (coro/resume co) :done) "deep chain: final"))

# ============================================================================
# Interleaved sustained coroutines
# ============================================================================

(begin
  (defn gen (start)
    (fn []
      (let [i 0]
        (while (< i 15)
          (yield (+ start i))
          (assign i (+ i 1)))
        (+ start 100))))
  (var co-a (make-coroutine (gen 0)))
  (var co-b (make-coroutine (gen 1000)))
  (let [i 0]
    (while (< i 15)
      (assert (= (coro/resume co-a) (+ 0 i))
              (string "interleaved A: " i))
      (assert (= (coro/resume co-b) (+ 1000 i))
              (string "interleaved B: " i))
      (assign i (+ i 1))))
  (assert (= (coro/resume co-a) 100) "interleaved A: final")
  (assert (= (coro/resume co-b) 1100) "interleaved B: final"))

# ============================================================================
# Yield-through-call with resume values (the tricky case)
# ============================================================================

(begin
  (defn yielder2 (x) (yield x))
  (defn wrapper (x)
    (let [result (yielder2 x)]
      (+ result 1)))
  (var co (make-coroutine (fn []
    (let [a (wrapper 10)]
      (let [b (wrapper 20)]
        (list a b))))))
  (assert (= (coro/resume co) 10) "yield-through-call resume: first yield")
  (assert (= (coro/resume co 100) 20) "yield-through-call resume: second yield")
  (assert (= (coro/resume co 200) (list 101 201)) "yield-through-call resume: final"))

# ============================================================================
# SIG_IO inside coroutine body (print emits SIG_IO which propagates
# out of coroutine since mask=SIG_YIELD doesn't catch SIG_IO)
# ============================================================================

(begin
  (defn yielder3 (x) (yield x))
  (defn wrapper3 (x)
    (let [result (yielder3 x)]
      # print emits SIG_IO — propagates through coroutine to scheduler
      (print (string "wrapper3: result=" result))
      (+ result 1)))
  (var co (make-coroutine (fn []
    (let [a (wrapper3 10)]
      (let [b (wrapper3 20)]
        (list a b))))))
  (assert (= (coro/resume co) 10) "IO-in-coroutine: first yield")
  (assert (= (coro/resume co 100) 20) "IO-in-coroutine: second yield")
  (assert (= (coro/resume co 200) (list 101 201)) "IO-in-coroutine: final"))

# ============================================================================
# Coroutine with print wrapping coro/resume in caller
# ============================================================================

(begin
  (defn yielder4 (x) (yield x))
  (var co (make-coroutine (fn []
    (yielder4 10)
    (yielder4 20)
    :done)))
  (let [v1 nil v2 nil v3 nil]
    (assign v1 (coro/resume co))
    (assert (= v1 10) "print-around-resume: first yield")
    (assign v2 (coro/resume co 100))
    (assert (= v2 20) "print-around-resume: second yield")
    (assign v3 (coro/resume co 200))
    (assert (= v3 :done) "print-around-resume: final")))

# ============================================================================
# Sustained yield with JIT interaction (hotness threshold is 10)
# ============================================================================

(begin
  (defn hot-yielder (x) (yield x))
  (var co (make-coroutine (fn []
    (let [i 0]
      (while (< i 25)
        (hot-yielder (* i 10))
        (assign i (+ i 1)))
      :done))))
  (let [i 0]
    (while (< i 25)
      (let [v (coro/resume co)]
        (assert (= v (* i 10))
                (string "JIT sustained: yield " i " expected " (* i 10) " got " v)))
      (assign i (+ i 1))))
  (assert (= (coro/resume co) :done) "JIT sustained: final"))
