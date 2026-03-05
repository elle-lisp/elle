## Coroutine Tests
##
## Tests for Elle's colorless coroutine implementation.
## Covers basic yield/resume, state management, and effect inference.

(import-file "./examples/assertions.lisp")

## ============================================================================
## BATCH 1: Basic Yield/Resume and Simple State Checks (10 tests)
## ============================================================================

## Test 1: Simple yield returns yielded value
(let ((co (make-coroutine (fn () (yield 42)))))
  (let ((result (coro/resume co)))
    (assert-eq result 42 "Simple yield should return 42")))

## Test 2: Multiple yields return each value in sequence
(let ((co (make-coroutine (fn () (yield 1) (yield 2) (yield 3) 4))))
  (let ((v1 (coro/resume co)))
    (let ((v2 (coro/resume co)))
      (let ((v3 (coro/resume co)))
        (let ((v4 (coro/resume co)))
          (assert-eq v1 1 "First yield should be 1")
          (assert-eq v2 2 "Second yield should be 2")
          (assert-eq v3 3 "Third yield should be 3")
          (assert-eq v4 4 "Fourth resume should return 4"))))))

## Test 3: Resume with value becomes yield expression result
(let ((co (make-coroutine (fn () (+ 10 (yield 1))))))
  (let ((v1 (coro/resume co)))
    (let ((v2 (coro/resume co 5)))
      (assert-eq v1 1 "First resume yields 1")
      (assert-eq v2 15 "Second resume with 5 returns 10+5=15"))))

## Test 4: Coroutine status created
(let ((co (make-coroutine (fn () 42))))
  (let ((status (keyword->string (coro/status co))))
    (assert-eq status "created" "Initial status should be 'created'")))

## Test 5: Coroutine status suspended after yield
(let ((co (make-coroutine (fn () (yield 1) (yield 2)))))
  (begin
    (coro/resume co)
    (let ((status (keyword->string (coro/status co))))
      (assert-eq status "suspended" "Status after yield should be 'suspended'"))))

## Test 6: Coroutine status done after completion
(let ((co (make-coroutine (fn () 42))))
  (begin
    (coro/resume co)
    (let ((status (keyword->string (coro/status co))))
      (assert-eq status "done" "Status after completion should be 'done'"))))

## Test 7: Coroutine done? predicate
(let ((co (make-coroutine (fn () 42))))
  (let ((before (coro/done? co)))
    (begin
      (coro/resume co)
      (let ((after (coro/done? co)))
        (assert-false before "done? should be false initially")
        (assert-true after "done? should be true after completion")))))

## Test 8: Coroutine value after yield
(let ((co (make-coroutine (fn () (yield 42)))))
  (begin
    (coro/resume co)
    (let ((val (coro/value co)))
      (assert-eq val 42 "coro/value should return last yielded value"))))

## Test 9: Pure function without yield works normally
(letrec ((sum (fn (n)
  (if (<= n 0)
    0
    (+ n (sum (- n 1)))))))
  (let ((result (sum 5)))
    (assert-eq result 15 "Pure recursive function should work normally")))

## Test 10: Coroutine with empty body
(let ((co (make-coroutine (fn () nil))))
  (let ((result (coro/resume co)))
    (assert-eq result nil "Empty body should return nil")))

## ============================================================================
## BATCH 2: Effect Inference and Yielding Functions (10 tests)
## ============================================================================

## Test 11: Yielding function detected
(let ((gen (fn () (yield 1) (yield 2))))
  (let ((co (make-coroutine gen)))
    (let ((result (coro/resume co)))
      (assert-eq result 1 "Yielding function should yield 1"))))

## Test 12: Calling yielding function propagates effect
(let ((f (fn () (yield 1))))
  (let ((g (fn () (f) (yield 2))))
    (let ((co (make-coroutine g)))
      (let ((result (coro/resume co)))
        (assert-eq result 1 "Inner function's yield should propagate")))))

## Test 13: Coroutine with captured variables
(let ((x 10))
  (let ((co (make-coroutine (fn () (yield x)))))
    (let ((result (coro/resume co)))
      (assert-eq result 10 "Captured variable should be accessible"))))

## Test 14: Coroutine with multiple captured variables
(let ((x 10) (y 20))
  (let ((co (make-coroutine (fn () (yield (+ x y))))))
    (let ((result (coro/resume co)))
      (assert-eq result 30 "Multiple captured variables should work"))))

## Test 15: Coroutine captures mutable state
(let ((counter (box 0)))
  (let ((co (make-coroutine (fn ()
    (rebox counter (+ (unbox counter) 1))
    (yield (unbox counter))))))
    (let ((result (coro/resume co)))
      (assert-eq result 1 "Mutable cell should be updated"))))

## Test 16: Nested coroutines
(let ((inner-gen (fn () (yield 10))))
  (let ((outer-gen (fn ()
    (let ((inner-co (make-coroutine inner-gen)))
      (yield (coro/resume inner-co))))))
    (let ((co (make-coroutine outer-gen)))
      (let ((result (coro/resume co)))
        (assert-eq result 10 "Nested coroutine should work")))))

## Test 17: Nested coroutines multiple levels
(let ((level3 (fn () (yield 3))))
  (let ((level2 (fn ()
    (let ((co3 (make-coroutine level3)))
      (yield (coro/resume co3))))))
    (let ((level1 (fn ()
      (let ((co2 (make-coroutine level2)))
        (yield (coro/resume co2))))))
      (let ((co1 (make-coroutine level1)))
        (let ((result (coro/resume co1)))
          (assert-eq result 3 "Three-level nesting should work"))))))

## Test 18: Coroutine with no yield
(let ((co (make-coroutine (fn () 42))))
  (let ((result (coro/resume co)))
    (assert-eq result 42 "Coroutine without yield should return value")))

## Test 19: Coroutine with nil yield
(let ((co (make-coroutine (fn () (yield nil)))))
  (let ((result (coro/resume co)))
    (assert-eq result nil "Yielding nil should work")))

## Test 20: Coroutine with complex yielded value
(let ((co (make-coroutine (fn () (yield (list 1 2 3))))))
  (let ((result (coro/resume co)))
    (assert-not-nil result "Complex value should be yielded")))

## ============================================================================
## BATCH 3: Error Handling and State Management (10 tests)
## ============================================================================

## Test 21: Resume done coroutine fails
(let ((co (make-coroutine (fn () 42))))
  (begin
    (coro/resume co)
    (assert-err (fn () (coro/resume co)) "Resuming done coroutine should error")))

## Test 22: Error in coroutine
(let ((co (make-coroutine (fn () (/ 1 0)))))
  (assert-err (fn () (coro/resume co)) "Division by zero should error"))

## Test 23: Coroutine predicate
(let ((co (make-coroutine (fn () 42))))
  (let ((is-coro (coro? co)))
    (assert-true is-coro "coro? should return true for coroutine")))

## Test 24: Non-coroutine fails predicate
(let ((is-coro (coro? 42)))
  (assert-false is-coro "coro? should return false for non-coroutine"))

## Test 25: Coroutine with recursion
(letrec ((countdown (fn (n)
  (if (<= n 0)
    (yield 0)
    (begin
      (yield n)
      (countdown (- n 1)))))))
  (let ((co (make-coroutine (fn () (countdown 3)))))
    (let ((result (coro/resume co)))
      (assert-eq result 3 "Recursive coroutine should yield 3"))))

## Test 26: Coroutine with higher-order functions
(let ((co (make-coroutine (fn ()
  (yield (map (fn (x) (* x 2)) (list 1 2 3)))))))
  (let ((result (coro/resume co)))
    (assert-not-nil result "Higher-order function in coroutine should work")))

## Test 27: Yield in if expression (true branch)
(let ((gen (fn ()
  (if true
    (yield 1)
    (yield 2)))))
  (let ((co (make-coroutine gen)))
    (let ((result (coro/resume co)))
      (assert-eq result 1 "Yield in true branch should work"))))

## Test 28: Yield in if expression (false branch)
(let ((gen (fn ()
  (if false
    (yield 1)
    (yield 2)))))
  (let ((co (make-coroutine gen)))
    (let ((result (coro/resume co)))
      (assert-eq result 2 "Yield in false branch should work"))))

## Test 29: Yield in begin expression
(let ((gen (fn ()
  (begin
    (yield 1)
    (yield 2)))))
  (let ((co (make-coroutine gen)))
    (let ((result (coro/resume co)))
      (assert-eq result 1 "Yield in begin should work"))))

## Test 30: Yield with computation
(let ((gen (fn ()
  (yield (+ 10 20 12)))))
  (let ((co (make-coroutine gen)))
    (let ((result (coro/resume co)))
      (assert-eq result 42 "Computed yield should work"))))

## ============================================================================
## BATCH 4: Yield in Let and Complex Expressions (10 tests)
## ============================================================================

## Test 31: Yield in let expression
(let ((gen (fn ()
  (let ((x 10))
    (yield x)))))
  (let ((co (make-coroutine gen)))
    (let ((result (coro/resume co)))
      (assert-eq result 10 "Yield in let should work"))))

## Test 32: Yield with captured variable in let
(let ((x 42))
  (let ((gen (fn () (yield x))))
    (let ((co (make-coroutine gen)))
      (let ((result (coro/resume co)))
        (assert-eq result 42 "Captured variable in let should work")))))

## Test 33: Yield in and expression
(let ((gen (fn ()
  (and true (yield 42)))))
  (let ((co (make-coroutine gen)))
    (let ((result (coro/resume co)))
      (assert-eq result 42 "Yield in and should work"))))

## Test 34: Yield in or expression
(let ((gen (fn ()
  (or false (yield 42)))))
  (let ((co (make-coroutine gen)))
    (let ((result (coro/resume co)))
      (assert-eq result 42 "Yield in or should work"))))

## Test 35: Yield in cond expression
(let ((gen (fn ()
  (cond
    (false (yield 1))
    (true (yield 2))
    (else (yield 3))))))
  (let ((co (make-coroutine gen)))
    (let ((result (coro/resume co)))
      (assert-eq result 2 "Yield in cond should work"))))

## Test 36: Yield with intermediate values on stack
(let ((co (make-coroutine (fn () (+ 1 (yield 2) 3)))))
  (let ((v1 (coro/resume co)))
    (let ((v2 (coro/resume co 10)))
      (assert-eq v1 2 "First yield should be 2")
      (assert-eq v2 14 "Second resume with 10 should return 1+10+3=14"))))

## Test 37: Yield with multiple intermediate values
(let ((co (make-coroutine (fn () (+ 1 2 (yield 3) 4 5)))))
  (let ((v1 (coro/resume co)))
    (let ((v2 (coro/resume co 100)))
      (assert-eq v1 3 "First yield should be 3")
      (assert-eq v2 112 "Second resume should return 1+2+100+4+5=112"))))

## Test 38: Yield in nested call with intermediate values
(let ((co (make-coroutine (fn () (* 2 (+ 1 (yield 5) 3))))))
  (let ((v1 (coro/resume co)))
    (let ((v2 (coro/resume co 10)))
      (assert-eq v1 5 "First yield should be 5")
      (assert-eq v2 28 "Second resume should return 2*(1+10+3)=28"))))

## Test 39: Multiple yields with intermediate values
(let ((co (make-coroutine (fn ()
  (+ (+ 1 (yield 2) 3)
     (+ 4 (yield 5) 6))))))
  (let ((v1 (coro/resume co)))
    (let ((v2 (coro/resume co 10)))
      (let ((v3 (coro/resume co 20)))
        (assert-eq v1 2 "First yield should be 2")
        (assert-eq v2 5 "Second yield should be 5")
        (assert-eq v3 44 "Final result should be (1+10+3)+(4+20+6)=44")))))

## Test 40: Make-coroutine with pure closure
(let ((co (make-coroutine (fn () 42))))
  (let ((result (coro/resume co)))
    (assert-eq result 42 "Pure closure in coroutine should work")))

## ============================================================================
## BATCH 5: Quoted Symbols and Literal Values (10 tests)
## ============================================================================

## Test 41: Yield quoted symbol
(let ((gen (fn () (yield 'a) (yield 'b) (yield 'c))))
  (let ((co (make-coroutine gen)))
    (let ((v1 (coro/resume co)))
      (let ((v2 (coro/resume co)))
        (let ((v3 (coro/resume co)))
          (assert-true (symbol? v1) "First yield should be symbol")
          (assert-true (symbol? v2) "Second yield should be symbol")
          (assert-true (symbol? v3) "Third yield should be symbol"))))))

## Test 42: Yield quoted symbol is value not variable
(let ((gen (fn () (yield 'test-symbol))))
  (let ((co (make-coroutine gen)))
    (let ((result (coro/resume co)))
      (assert-true (symbol? result) "Yielded quoted symbol should be symbol value"))))

## Test 43: Yield various literal types
(let ((gen (fn ()
  (yield 'symbol-val)
  (yield 42)
  (yield "string")
  (yield true)
  (yield nil))))
  (let ((co (make-coroutine gen)))
    (let ((v1 (coro/resume co)))
      (let ((v2 (coro/resume co)))
        (let ((v3 (coro/resume co)))
          (let ((v4 (coro/resume co)))
            (let ((v5 (coro/resume co)))
              (assert-true (symbol? v1) "First should be symbol")
              (assert-true (number? v2) "Second should be number")
              (assert-true (string? v3) "Third should be string")
              (assert-true v4 "Fourth should be true")
              (assert-eq v5 nil "Fifth should be nil"))))))))

## Test 44: Yield quoted list
(let ((gen (fn () (yield '(1 2 3)))))
  (let ((co (make-coroutine gen)))
    (let ((result (coro/resume co)))
      (assert-not-nil result "Quoted list should be yielded"))))

## Test 45: Yield keyword
(let ((gen (fn () (yield :keyword))))
  (let ((co (make-coroutine gen)))
    (let ((result (coro/resume co)))
      (assert-not-nil result "Keyword should be yielded"))))

## Test 46: Yield boolean true
(let ((gen (fn () (yield true))))
  (let ((co (make-coroutine gen)))
    (let ((result (coro/resume co)))
      (assert-true result "Boolean true should be yielded"))))

## Test 47: Yield boolean false
(let ((gen (fn () (yield false))))
  (let ((co (make-coroutine gen)))
    (let ((result (coro/resume co)))
      (assert-false result "Boolean false should be yielded"))))

## Test 48: Yield string
(let ((gen (fn () (yield "hello"))))
  (let ((co (make-coroutine gen)))
    (let ((result (coro/resume co)))
      (assert-string-eq result "hello" "String should be yielded"))))

## Test 49: Yield float
(let ((gen (fn () (yield 3.14))))
  (let ((co (make-coroutine gen)))
    (let ((result (coro/resume co)))
      (assert-not-nil result "Float should be yielded"))))

## Test 50: Yield large number
(let ((gen (fn () (yield 999999))))
  (let ((co (make-coroutine gen)))
    (let ((result (coro/resume co)))
      (assert-eq result 999999 "Large number should be yielded"))))

## ============================================================================
## BATCH 6: Multiple Coroutines and Interleaving (10 tests)
## ============================================================================

## Test 51: Multiple independent coroutines
(let ((co1 (make-coroutine (fn () (yield 1)))))
  (let ((co2 (make-coroutine (fn () (yield 2)))))
    (let ((v1 (coro/resume co1)))
      (let ((v2 (coro/resume co2)))
        (assert-eq v1 1 "First coroutine should yield 1")
        (assert-eq v2 2 "Second coroutine should yield 2")))))

## Test 52: Interleaved resume operations
(let ((co1 (make-coroutine (fn () (yield 1) (yield 2) (yield 3)))))
  (let ((co2 (make-coroutine (fn () (yield 10) (yield 20) (yield 30)))))
    (let ((v1 (coro/resume co1)))
      (let ((v2 (coro/resume co2)))
        (let ((v3 (coro/resume co1)))
          (let ((v4 (coro/resume co2)))
            (assert-eq v1 1 "co1 first yield")
            (assert-eq v2 10 "co2 first yield")
            (assert-eq v3 2 "co1 second yield")
            (assert-eq v4 20 "co2 second yield")))))))

## Test 53: Multiple coroutines independent state
(let ((gen1 (fn () (yield 'a) (yield 'b))))
  (let ((gen2 (fn () (yield 'x) (yield 'y))))
    (let ((co1 (make-coroutine gen1)))
      (let ((co2 (make-coroutine gen2)))
        (let ((s1 (keyword->string (coro/status co1))))
          (let ((s2 (keyword->string (coro/status co2))))
            (assert-eq s1 "created" "co1 initial status")
            (assert-eq s2 "created" "co2 initial status")))))))

## Test 54: Coroutine status independent
(let ((gen1 (fn () (yield 1) (yield 2))))
  (let ((gen2 (fn () (yield 10) (yield 20))))
    (let ((co1 (make-coroutine gen1)))
      (let ((co2 (make-coroutine gen2)))
        (begin
          (coro/resume co1)
          (let ((s1 (keyword->string (coro/status co1))))
            (let ((s2 (keyword->string (coro/status co2))))
              (assert-eq s1 "suspended" "co1 should be suspended")
              (assert-eq s2 "created" "co2 should still be created"))))))))

## Test 55: Nested coroutine resume from coroutine
(let ((inner-gen (fn () (yield 10) (yield 20))))
  (let ((outer-gen (fn ()
    (let ((inner-co (make-coroutine inner-gen)))
      (yield (+ 1 (coro/resume inner-co)))
      (yield (+ 1 (coro/resume inner-co)))))))
    (let ((outer-co (make-coroutine outer-gen)))
      (let ((v1 (coro/resume outer-co)))
        (let ((v2 (coro/resume outer-co)))
          (assert-eq v1 11 "First nested yield should be 11")
          (assert-eq v2 21 "Second nested yield should be 21"))))))

## Test 56: Coroutine with large yielded value
(let ((co (make-coroutine (fn ()
  (yield (list 1 2 3 4 5 6 7 8 9 10))))))
  (let ((result (coro/resume co)))
    (assert-not-nil result "Large list should be yielded")))

## Test 57: Closure captured var after resume (issue 258)
(let ((make-counter (fn (start)
  (fn ()
    (yield start)
    (yield (+ start 1))
    (yield (+ start 2))))))
  (let ((co-100 (make-coroutine (make-counter 100))))
    (let ((v1 (coro/resume co-100)))
      (let ((v2 (coro/resume co-100)))
        (let ((v3 (coro/resume co-100)))
          (assert-eq v1 100 "First yield should be 100")
          (assert-eq v2 101 "Second yield should be 101")
          (assert-eq v3 102 "Third yield should be 102"))))))

## Test 58: Interleaved coroutines issue 259
(let ((make-counter (fn (start)
  (fn ()
    (yield start)
    (yield (+ start 1))
    (yield (+ start 2))))))
  (let ((co-100 (make-coroutine (make-counter 100))))
    (let ((co-200 (make-coroutine (make-counter 200))))
      (let ((v1 (coro/resume co-100)))
        (let ((v2 (coro/resume co-200)))
          (let ((v3 (coro/resume co-100)))
            (let ((v4 (coro/resume co-200)))
              (assert-eq v1 100 "co-100 first")
              (assert-eq v2 200 "co-200 first")
              (assert-eq v3 101 "co-100 second")
              (assert-eq v4 201 "co-200 second"))))))))

## Test 59: Coroutine done? after multiple yields
(let ((gen (fn () (yield 1) (yield 2) (yield 3) 4)))
  (let ((co (make-coroutine gen)))
    (begin
      (coro/resume co)
      (coro/resume co)
      (coro/resume co)
      (coro/resume co)
      (let ((done (coro/done? co)))
        (assert-true done "Should be done after all yields")))))

## Test 60: Coroutine value tracks last yield
(let ((gen (fn () (yield 10) (yield 20) (yield 30))))
  (let ((co (make-coroutine gen)))
    (begin
      (coro/resume co)
      (let ((v1 (coro/value co)))
        (coro/resume co)
        (let ((v2 (coro/value co)))
          (coro/resume co)
          (let ((v3 (coro/value co)))
            (assert-eq v1 10 "Value after first yield")
            (assert-eq v2 20 "Value after second yield")
            (assert-eq v3 30 "Value after third yield")))))))
