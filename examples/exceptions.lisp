;; Exception Handling in Elle Lisp

(import-file "./examples/assertions.lisp")

;; ============================================================================
;; Basic Exception Creation and Inspection
;; ============================================================================

;; Example 1: Create and inspect exceptions
(define exc1 (exception "Division by zero"))

;; Example 2: Exception with attached data
(define exc2 (exception "Invalid input" 42))

;; Example 3: Exception with complex data structure
(define error-details (list "field" "username" "reason" "taken"))
(define exc3 (exception "Validation error" error-details))

;; Example 4: Try block with successful execution
(define try-result (try (+ 10 20) (catch e "error")))
(assert-equal try-result 30 "Try block with successful execution")

;; Example 5: Try block returning the value
(define safe-value 
  (try 
    (* 5 7)
    (catch e 0)))
(assert-equal safe-value 35 "Try block returns computed value")

;; Example 6: Creating exception for later use
(define error-object (exception "Connection timeout" (list "host" "127" "port" 8080)))

;; Example 7: Multiple different exceptions
(define auth-error (exception "Authentication failed" "Invalid"))
(define network-error (exception "Network unreachable" (list "code" 503)))
(define parse-error (exception "JSON parse error" 15))

;; Example 8: Try blocks in computations
(define result1 (try (- 10 3) (catch e -1)))
(define result2 (try (/ 20 4) (catch e 0)))
(define result3 (try 100 (catch e 50)))

;; Example 9: Extract exception message
(exception-message exc1)

;; Example 10: Extract exception data
(exception-data exc2)

;; Example 11: Extract data from complex exception
(exception-data exc3)

;; Example 12: Extract data from network error
(exception-data network-error)

;; Example 13: Exception message from parse error
(exception-message parse-error)

;; Example 14: Result of arithmetic in try block
result1

;; Example 15: Another computed result
result2

;; Example 16: Third result
result3

;; ============================================================================
;; Safe Arithmetic Operations
;; ============================================================================

;; A safe wrapper that returns -1 on division by zero
(define safe-divide
  (fn (a b)
    (try 
      (/ a b)
      (catch e -1))))

(define div-result-1 (safe-divide 10 2))
(assert-equal div-result-1 5 "safe-divide: 10/2 = 5")
(define div-result-2 (safe-divide 20 0))
(assert-equal div-result-2 -1 "safe-divide: 20/0 returns -1")
(define div-result-3 (safe-divide 30 3))
(assert-equal div-result-3 10 "safe-divide: 30/3 = 10")

;; ============================================================================
;; Sequential Exception Handling
;; ============================================================================

;; Multiple exception-catching operations in sequence
(define seq-result1 (try (/ 100 0) (catch e 999)))
(define seq-result2 (try (/ 200 0) (catch e 888)))
(define seq-result3 (try (/ 300 0) (catch e 777)))

(assert-equal seq-result1 999 "Sequential exception 1: 100/0 returns 999")
(assert-equal seq-result2 888 "Sequential exception 2: 200/0 returns 888")
(assert-equal seq-result3 777 "Sequential exception 3: 300/0 returns 777")

;; ============================================================================
;; Mixed Success and Failure Operations
;; ============================================================================

;; Interleaving successful and exception-throwing operations
(define success1 (try (+ 10 20) (catch e 0)))
(define fail1 (try (/ 10 0) (catch e 99)))
(define success2 (try (* 5 6) (catch e 0)))
(define fail2 (try (/ 20 0) (catch e 88)))

(assert-equal success1 30 "Mixed operations: success1 = 10+20 = 30")
(assert-equal fail1 99 "Mixed operations: fail1 = 10/0 returns 99")
(assert-equal success2 30 "Mixed operations: success2 = 5*6 = 30")
(assert-equal fail2 88 "Mixed operations: fail2 = 20/0 returns 88")

;; ============================================================================
;; Exception Variable Capture
;; ============================================================================

;; Capture the exception value itself and use it
(define exc-capture1 (try (/ 10 0) (catch e e)))
(define exc-capture2 (try (/ 20 0) (catch e e)))

;; Both are condition objects - verify they are not nil
(assert-true (not (nil? exc-capture1)) "Exception capture 1: exc1 is not nil")
(assert-true (not (nil? exc-capture2)) "Exception capture 2: exc2 is not nil")

;; ============================================================================
;; Nested Function Calls with Exception Handling
;; ============================================================================

;; Function that handles exceptions at multiple levels
(define compute-safe
  (fn (x)
    (try 
      (/ 100 x)
      (catch e 0))))

(define wrapper-func
  (fn (x)
    (try 
      (compute-safe x)
      (catch e -1))))

(define nested-result-1 (wrapper-func 10))
(assert-equal nested-result-1 10 "Nested exception handling: 100/10 = 10")
(define nested-result-2 (wrapper-func 0))
(assert-equal nested-result-2 0 "Nested exception handling: 100/0 returns 0")
(define nested-result-3 (wrapper-func 5))
(assert-equal nested-result-3 20 "Nested exception handling: 100/5 = 20")

;; ============================================================================
;; Exception Handling in Conditional Logic
;; ============================================================================

;; Using exceptions in conditional expressions
(define check1 (if #t (try (/ 1 0) (catch e 100)) 0))
(define check2 (if #f 0 (try (/ 2 0) (catch e 200))))

(assert-equal check1 100 "Conditional exception: if true, catch exception returns 100")
(assert-equal check2 200 "Conditional exception: if false, catch exception returns 200")

;; ============================================================================
;; Sequential Catches with State Update
;; ============================================================================

;; Multiple exception catches that update state
(define state 0)

(define state-r1 (try (/ 10 0) (catch e (begin (set! state 1) 10))))
(define state-r2 (try (/ 20 0) (catch e (begin (set! state 2) 20))))
(define state-r3 (try (/ 30 0) (catch e (begin (set! state 3) 30))))

(assert-equal state-r1 10 "State update exception 1: returns 10")
(assert-equal state-r2 20 "State update exception 2: returns 20")
(assert-equal state-r3 30 "State update exception 3: returns 30")
(assert-equal state 3 "State update: final state is 3")

;; ============================================================================
;; Exception Handling with Lambda Functions
;; ============================================================================

;; Exception handling inside fn expressions
(define handler1 (fn () (try (/ 10 0) (catch e 999))))
(define handler2 (fn () (try (/ 20 0) (catch e 888))))
(define handler3 (fn () (try (/ 30 0) (catch e 777))))

;; Call each handler
(define h1-result (handler1))
(assert-equal h1-result 999 "Lambda exception handler 1: returns 999")
(define h2-result (handler2))
(assert-equal h2-result 888 "Lambda exception handler 2: returns 888")
(define h3-result (handler3))
(assert-equal h3-result 777 "Lambda exception handler 3: returns 777")

;; ============================================================================
;; Repeated Exception Catching Pattern
;; ============================================================================

;; Demonstrate the bug is truly fixed: multiple exceptions in same scope
(define pattern-a (try (/ 100 10) (catch e 0)))
(define pattern-b (try (/ 100 0) (catch e 99)))
(define pattern-c (try (/ 100 20) (catch e 0)))
(define pattern-d (try (/ 100 0) (catch e 88)))
(define pattern-e (try (/ 100 25) (catch e 0)))

(assert-equal pattern-a 10 "Repeated pattern a: 100/10 = 10")
(assert-equal pattern-b 99 "Repeated pattern b: 100/0 returns 99")
(assert-equal pattern-c 5 "Repeated pattern c: 100/20 = 5")
(assert-equal pattern-d 88 "Repeated pattern d: 100/0 returns 88")
(assert-equal pattern-e 4 "Repeated pattern e: 100/25 = 4")

;; ============================================================================
;; Chain of Exception Handlers
;; ============================================================================

;; Create a chain of operations that each handle exceptions
(define chain-result-1 (try (/ 50 0) (catch e 111)))
(define chain-result-2 (try (+ chain-result-1 chain-result-1) (catch e 222)))
(define chain-result-3 (try (- chain-result-2 50) (catch e 333)))

(assert-equal chain-result-1 111 "Chain handler 1: 50/0 returns 111")
(assert-equal chain-result-2 222 "Chain handler 2: 111+111 = 222")
(assert-equal chain-result-3 172 "Chain handler 3: 222-50 = 172")

;; ============================================================================
;; Try/Catch/Finally Examples
;; ============================================================================

;; Verify finally blocks execute after sequential exceptions
(define finally-state 0)

(define finally-f1 (try (/ 10 0) (catch e 1) (finally (set! finally-state 1))))
(define finally-f2 (try (/ 20 0) (catch e 2) (finally (set! finally-state 2))))
(define finally-f3 (try (/ 30 0) (catch e 3) (finally (set! finally-state 3))))

(assert-equal finally-f1 1 "Finally block 1: returns 1")
(assert-equal finally-f2 2 "Finally block 2: returns 2")
(assert-equal finally-f3 3 "Finally block 3: returns 3")
(assert-equal finally-state 3 "Finally block: state updated to 3")

;; Finally block always executes
(try 
  (begin
    (display "Try body executed")
    (newline))
  (finally (begin
    (display "Finally block executed")
    (newline))))

;; Try without exception
(define try-no-exc (try (+ 5 3)))
(assert-equal try-no-exc 8 "Try without exception: 5 + 3 should be 8")

;; Catch ignored on success
(define try-catch-success (try (+ 5 3) (catch e 999)))
(assert-equal try-catch-success 8 "Catch not executed on success, result should be 8")

;; ============================================================================
;; Safe Arithmetic with Error Hierarchy
;; ============================================================================

;; Safe operation that handles errors gracefully
(define safe-error-handler
  (fn (x y)
    "Demonstrates error handling at different levels"
    (if (= y 0)
      0
      (/ x y))))

;; Test the safe operation
(define safe-result-1 (safe-error-handler 100 5))
(assert-equal safe-result-1 20 "safe-error-handler: 100/5 = 20")
(define safe-result-2 (safe-error-handler 50 0))
(assert-equal safe-result-2 0 "safe-error-handler: 50/0 protected returns 0")

;; Safe arithmetic chain
(define safe-complex-calc
  (fn (a b c)
    "Complex calculation that could fail at multiple levels"
    (if (= c 0)
      0
      (* (+ a b) (/ 100 c)))))

(define complex-result-1 (safe-complex-calc 10 20 4))
(assert-equal complex-result-1 750 "safe-complex-calc: (10+20)*(100/4) = 750")
(define complex-result-2 (safe-complex-calc 5 5 0))
(assert-equal complex-result-2 0 "safe-complex-calc: c=0 protected returns 0")

;; More complex safe arithmetic
(define safe-complex-operation
  (fn (a b c)
    "Demonstrate safe operation with potential multiple failures"
    (if (= b 0)
      0
      (if (= c 0)
        (+ a (/ 100 b))
        (* (+ a b) (/ 100 c))))))

(define complex-op-1 (safe-complex-operation 10 5 2))
(assert-equal complex-op-1 750 "safe-complex-operation: (10+5)*(100/2) = 750")
(define complex-op-2 (safe-complex-operation 10 0 2))
(assert-equal complex-op-2 0 "safe-complex-operation: b=0 returns 0")
(define complex-op-3 (safe-complex-operation 10 5 0))
(assert-equal complex-op-3 30 "safe-complex-operation: c=0 returns 10+20 = 30")

;; Final inheritance tests
(define result-inheritance-1 (safe-error-handler 30 3))
(define result-inheritance-2 (safe-complex-calc 15 25 5))

(assert-equal result-inheritance-1 10 "Inheritance test 1: 30/3 = 10")
(assert-equal result-inheritance-2 800 "Inheritance test 2: (15+25)*(100/5) = 800")

;; ============================================================================
;; FINALLY CLAUSE SEMANTICS
;; ============================================================================

(display "\n")
(display "========================================\n")
(display "FINALLY CLAUSE SEMANTICS\n")
(display "========================================\n")

;; Example 1: Basic finally block
;; Finally ensures cleanup code runs regardless of success/failure
(display "\nExample 1: Basic finally")
(newline)
(define result1 (try 
  (+ 10 20)
  (finally 
    (display "  Cleanup executed"))))
(newline)
(assert-equal result1 30 "Try body result should be 30")
(newline)

;; Example 2: Finally doesn't change return value
;; The try body's result is returned, finally's result is ignored
(display "Example 2: Finally preserves try value")
(newline)
(display "  Result: ")
(define result2 (try 42 (finally 999)))
(display result2)
(newline)
(assert-equal result2 42 "Finally should not change return value")
(newline)

;; Example 3: Finally with side effects
;; Finally blocks can perform operations like display without affecting return
(display "Example 3: Finally with side effects")
(newline)
(display "  Value: ")
(define result3 (try 100 (finally (display "[cleanup]"))))
(display result3)
(newline)
(assert-equal result3 100 "Try value should be 100 despite finally side effects")
(newline)

;; Example 4: Nested finally blocks
;; Multiple levels of finally all execute
(display "Example 4: Nested finally blocks")
(newline)
(display "  Result: ")
(define result4 (try 
           (try 10 
                (finally 
                  (display "[inner-cleanup]")))
           (finally 
             (display "[outer-cleanup]"))))
(display result4)
(newline)
(assert-equal result4 10 "Nested finally should preserve inner try value")
(newline)

;; Example 5: Finally with simple expressions
;; Finally blocks can contain multiple expressions
(display "Example 5: Finally with expressions")
(newline)
(display "  Result: ")
(define result5 (try 55
              (finally (begin (+ 1 2) (* 3 4) 0))))
(display result5)
(newline)
(assert-equal result5 55 "Try value should be 55 despite finally expressions")
(newline)

;; Example 6: Finally with list operations
;; Finally can contain complex expressions
(display "Example 6: Finally with list operations")
(newline)
(display "  Result: ")
(display (try (list 1 2 3) 
              (finally (list 4 5 6))))
(newline)
(newline)

;; Example 7: Finally with arithmetic
;; Finally's computed value is discarded
(display "Example 7: Finally with arithmetic")
(newline)
(display "  Result: ")
(display (try 50 
              (finally (* 10 20))))  ;; Result 200 is ignored
(newline)
(newline)

;; Example 8: Try/catch/finally together
;; All three clauses work together
(display "Example 8: Try/catch/finally")
(newline)
(display "  Result: ")
(display (try 30
              (catch e 0)
              (finally (display "[cleanup]"))))
(newline)
(newline)

;; Example 9: Finally with conditional logic
;; Finally blocks can contain if expressions
(display "Example 9: Finally with conditional")
(newline)
(display "  Result: ")
(display (try 77
              (finally (if (> 5 3)
                         (display "[if-true]")
                         (display "[if-false]")))))
(newline)
(newline)

;; Example 10: Finally in sequence
;; Multiple try/finally expressions can be chained
(display "Example 10: Finally in sequence")
(newline)
(display "  First: ")
(display (try 1 (finally 0)))
(display ", Second: ")
(display (try 2 (finally 0)))
(display ", Third: ")
(display (try 3 (finally 0)))
(newline)
(newline)

;; Example 11: Finally with display and newline
;; Practical cleanup pattern
(display "Example 11: Cleanup pattern")
(newline)
(try
  (display "  Processing started")
  (finally
    (begin
      (newline)
      (display "  Processing complete"))))
(newline)
(newline)

;; Example 12: Finally with computation in try and finally
;; Both try and finally have complex expressions
(display "Example 12: Complex try and finally")
(newline)
(display "  Result: ")
(display (try 
           (+ (* 2 5) (* 3 4))  ;; 10 + 12 = 22
           (finally 
             (+ 100 200))))      ;; 300 ignored
(newline)
(newline)

;; ============================================================================
;; Assertion Summary
;; ============================================================================
(display "=== Exception Handling and Finally Clause Assertions Complete ===")
(newline)
