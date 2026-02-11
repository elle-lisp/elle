#!/usr/bin/env elle
;;; Advanced Exception Handling Examples
;;; Demonstrates sophisticated exception handling patterns fixed in Issue #162

;;; ============================================================================
;;; 1. Safe Arithmetic Operations
;;; ============================================================================

;; A safe wrapper that returns -1 on division by zero
(define safe-divide
  (fn (a b)
    (try 
      (/ a b)
      (catch e -1))))

(safe-divide 10 2)
(safe-divide 20 0)
(safe-divide 30 3)

;;; ============================================================================
;;; 2. Sequential Exception Handling (Issue #162 Regression)
;;; ============================================================================
;;; This used to fail with: "Cannot call <condition: id=4>"
;;; Now it correctly handles multiple sequential exceptions

;; Multiple exception-catching operations in sequence
(define result1 (try (/ 100 0) (catch e 999)))
(define result2 (try (/ 200 0) (catch e 888)))
(define result3 (try (/ 300 0) (catch e 777)))

result1
result2
result3

;;; ============================================================================
;;; 3. Mixed Success and Failure Operations
;;; ============================================================================

;; Interleaving successful and exception-throwing operations
(define success1 (try (+ 10 20) (catch e 0)))
(define fail1 (try (/ 10 0) (catch e 99)))
(define success2 (try (* 5 6) (catch e 0)))
(define fail2 (try (/ 20 0) (catch e 88)))

success1
fail1
success2
fail2

;;; ============================================================================
;;; 4. Exception Variable Capture
;;; ============================================================================

;; Capture the exception value itself and use it
(define exc1 (try (/ 10 0) (catch e e)))
(define exc2 (try (/ 20 0) (catch e e)))

;; Both are condition objects
exc1
exc2

;;; ============================================================================
;;; 5. Nested Function Calls with Exception Handling
;;; ============================================================================

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

(wrapper-func 10)
(wrapper-func 0)
(wrapper-func 5)

;;; ============================================================================
;;; 6. Exception Handling in Conditional Logic
;;; ============================================================================

;; Using exceptions in conditional expressions
(define check1 (if #t (try (/ 1 0) (catch e 100)) 0))
(define check2 (if #f 0 (try (/ 2 0) (catch e 200))))

check1
check2

;;; ============================================================================
;;; 7. Sequential Catches with State Update
;;; ============================================================================

;; Multiple exception catches that update state
(define state 0)

(define r1 (try (/ 10 0) (catch e (begin (set! state 1) 10))))
(define r2 (try (/ 20 0) (catch e (begin (set! state 2) 20))))
(define r3 (try (/ 30 0) (catch e (begin (set! state 3) 30))))

r1
r2
r3
state

;;; ============================================================================
;;; 8. Exception Handling with Lambda Functions
;;; ============================================================================

;; Exception handling inside fn expressions
(define handler1 (fn () (try (/ 10 0) (catch e 999))))
(define handler2 (fn () (try (/ 20 0) (catch e 888))))
(define handler3 (fn () (try (/ 30 0) (catch e 777))))

;; Call each handler
(handler1)
(handler2)
(handler3)

;;; ============================================================================
;;; 9. Repeated Exception Catching Pattern
;;; ============================================================================

;; Demonstrate the bug is truly fixed: multiple exceptions in same scope
(define result-a (try (/ 100 10) (catch e 0)))
(define result-b (try (/ 100 0) (catch e 99)))
(define result-c (try (/ 100 20) (catch e 0)))
(define result-d (try (/ 100 0) (catch e 88)))
(define result-e (try (/ 100 25) (catch e 0)))

result-a
result-b
result-c
result-d
result-e

;;; ============================================================================
;;; 10. Chain of Exception Handlers
;;; ============================================================================

;; Create a chain of operations that each handle exceptions
(define chain-result-1 (try (/ 50 0) (catch e 111)))
(define chain-result-2 (try (+ chain-result-1 chain-result-1) (catch e 222)))
(define chain-result-3 (try (- chain-result-2 50) (catch e 333)))

chain-result-1
chain-result-2
chain-result-3

;;; ============================================================================
;;; 11. Exception Handling with Try-Catch-Finally
;;; ============================================================================

;; Verify finally blocks execute after sequential exceptions
(define finally-state 0)

(define f1 (try (/ 10 0) (catch e 1) (finally (set! finally-state 1))))
(define f2 (try (/ 20 0) (catch e 2) (finally (set! finally-state 2))))
(define f3 (try (/ 30 0) (catch e 3) (finally (set! finally-state 3))))

f1
f2
f3
finally-state

;;; ============================================================================
;;; Core Bug Fix Verification: Issue #162
;;; ============================================================================
;;; Sequential exception-catching used to fail with:
;;;   "Cannot call <condition: id=4>"
;;; This was caused by incorrect variable binding in BindException instruction
;;; The fix: properly extract SymbolId from constants instead of using constant index

;; The exact test case from Issue #162
(define test1 (try (/ 10 0) (catch e 99)))
(define test2 (try (/ 20 0) (catch e 88)))

test1
test2

;;; Both should print without error - proving Issue #162 is fixed!
