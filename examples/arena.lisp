# Arena Memory Tracking
#
# Demonstrates heap arena introspection via (arena/count)
# and (arena/allocs). These tools enable precise measurement
# of heap allocation costs.
#
# (arena/count) returns the current arena object count as a bare
# integer with 1 object of overhead (the SIG_QUERY cons cell).
# (arena/allocs thunk) compensates and returns net allocations.

(import-file "./examples/assertions.lisp")

# ========================================
# 1. Basic arena stats
# ========================================
(display "=== 1. Basic arena stats ===\n")

(let ((stats (arena/stats)))
  (assert-true (>= (get stats :count) 0) "arena count is non-negative")
  (assert-true (>= (get stats :capacity) (get stats :count)) "capacity >= count"))

(let ((c (arena/count)))
  (assert-true (number? c) "arena/count returns a number")
  (assert-true (> c 0) "arena/count is positive after stdlib init")
  (display "  arena has ") (display c) (display " objects\n"))

(display "  ✓ basic stats work\n")

# ========================================
# 2. Measurement overhead
# ========================================
(display "\n=== 2. Measurement overhead ===\n")

# Each (arena/count) call has 1 object of overhead (SIG_QUERY cons)
(let* ((a (arena/count))
       (b (arena/count))
       (c (arena/count)))
  (assert-eq (- b a) 1 "arena/count overhead is 1")
  (assert-eq (- c b) 1 "arena/count overhead is stable"))

(display "  arena/count overhead: 1 object per call\n")

# arena/allocs compensates for this
(let* ((m (arena/allocs (fn () nil)))
       (allocs (first (rest m))))
  (assert-eq allocs 0 "nil thunk allocates 0 net objects")
  (display "  arena/allocs overhead: compensated to 0\n"))

(display "  ✓ overhead is understood\n")

# ========================================
# 3. Allocation costs per type
# ========================================
(display "\n=== 3. Allocation costs ===\n")

(defn net-allocs (thunk)
  (first (rest (arena/allocs thunk))))

(let ((n (net-allocs (fn () (cons 1 2)))))
  (assert-eq n 1 "cons = 1 heap object")
  (display "  cons:         ") (display n) (newline))

(let ((n (net-allocs (fn () (list 1 2 3 4 5)))))
  (assert-eq n 5 "list of 5 = 5 cons cells")
  (display "  (list 1..5):  ") (display n) (newline))

(let ((n (net-allocs (fn () (fn (x) x)))))
  (assert-eq n 1 "closure = 1 heap object")
  (display "  closure:      ") (display n) (newline))

(let ((n (net-allocs (fn () @[1 2 3]))))
  (assert-eq n 1 "array = 1 heap object")
  (display "  @[1 2 3]:     ") (display n) (newline))

(let ((n (net-allocs (fn () @{:a 1 :b 2}))))
  (assert-eq n 1 "table = 1 heap object")
  (display "  @{:a 1 :b 2}: ") (display n) (newline))

# String literals are in the constant pool, not runtime-allocated
(let ((n (net-allocs (fn () "a long string literal"))))
  (assert-eq n 0 "string literal = 0 (constant pool)")
  (display "  str literal:  ") (display n) (newline))

(display "  ✓ allocation costs verified\n")

# ========================================
# 4. Immediates are free
# ========================================
(display "\n=== 4. Immediates ===\n")

(assert-eq (net-allocs (fn () (+ 1 2))) 0 "int arithmetic = 0")
(assert-eq (net-allocs (fn () (< 1 2))) 0 "comparison = 0")
(assert-eq (net-allocs (fn () nil)) 0 "nil = 0")
(assert-eq (net-allocs (fn () :foo)) 0 "keyword = 0")

(display "  int, bool, nil, keyword: 0 objects each\n")
(display "  ✓ immediates verified\n")

# ========================================
# 5. Growth patterns
# ========================================
(display "\n=== 5. Growth patterns ===\n")

# Pure int loop: bounded
(let ((n (net-allocs (fn ()
           (letrec ((loop (fn (i acc)
                            (if (>= i 1000) acc
                              (loop (+ i 1) (+ acc i))))))
             (loop 0 0))))))
  (display "  1000 int additions: ") (display n) (display " objects\n")
  (assert-true (<= n 2) "int loop is bounded"))

# List creation: linear (expected without arena release)
(let ((n (net-allocs (fn ()
           (letrec ((loop (fn (i)
                            (when (< i 100)
                              (list 1 2 3)
                              (loop (+ i 1))))))
             (loop 0))))))
  (display "  100x (list 1 2 3): ") (display n) (display " objects\n")
  (assert-true (>= n 300) "list creation is linear"))

(display "  ✓ growth patterns verified\n")

# ========================================
# 6. Macro expansion: ArenaGuard works
# ========================================
(display "\n=== 6. Macro expansion stability ===\n")

# Per-iteration cost must be constant regardless of N.
# If ArenaGuard is working, macro temps are freed each expansion.
(defn measure-per-iter (n expr)
  (let* ((before (arena/count)))
    (letrec ((loop (fn (i)
                     (when (< i n)
                       (eval expr)
                       (loop (+ i 1))))))
      (loop 0))
    (/ (- (arena/count) before 1) n)))

(let* ((e '(let ((a 0)) (each x (list 1 2 3) (set a (+ a x))) a))
       (p10 (measure-per-iter 10 e))
       (p100 (measure-per-iter 100 e)))
  (display "  each via eval: ") (display p10) (display "/") (display p100)
  (display " per-iter (10x/100x)\n")
  (assert-eq p10 p100 "macro expansion cost is constant"))

(let* ((e '(defn temp (x) (let* ((a (+ x 1)) (b (+ a 2))) (-> b (* 2)))))
       (p10 (measure-per-iter 10 e))
       (p100 (measure-per-iter 100 e)))
  (display "  defn+let*+->: ") (display p10) (display "/") (display p100)
  (display " per-iter (10x/100x)\n")
  (assert-eq p10 p100 "complex macro cost is constant"))

(display "  ✓ ArenaGuard is working — no macro expansion leaks\n")

# ========================================
# 7. Fiber lifecycle
# ========================================
(display "\n=== 7. Fiber costs ===\n")

(let* ((p10 (measure-per-iter 10
              '(let ((f (fiber/new (fn () 42) 1)))
                 (fiber/resume f nil))))
       (p100 (measure-per-iter 100
               '(let ((f (fiber/new (fn () 42) 1)))
                  (fiber/resume f nil)))))
  (display "  fiber create+resume: ") (display p10) (display "/") (display p100)
  (display " per-iter\n")
  (assert-eq p10 p100 "fiber cost is constant"))

(display "  ✓ no fiber leaks\n")

# ========================================
# Summary
# ========================================
(display "\n========================================\n")
(display "All arena tracking tests passed!\n")
(display "========================================\n")
