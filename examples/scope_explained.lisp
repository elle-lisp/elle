;; Scope Management in Elle Lisp
;; This file demonstrates all scoping features and best practices

;; ============================================================================
;; PART 1: Global Scope
;; ============================================================================

;; Global variables are defined at the top level
(define global-x 100)
(define global-y 200)

;; They can be accessed anywhere
(display "Global x: ")
(display global-x)
(newline)

;; ============================================================================
;; PART 2: Function Scope (Local Variables)
;; ============================================================================

;; Parameters are local to the function
(define add-numbers (lambda (x y)
  (+ x y)))

(display "Add 3 + 4: ")
(display (add-numbers 3 4))
(newline)

;; Parameters shadow outer variables
(define x 1000)
(define shadowing-function (lambda (x)
  (+ x 1)))

(display "Inner x (1) + 1: ")
(display (shadowing-function 1))
(newline)
(display "Global x still: ")
(display x)
(newline)

;; ============================================================================
;; PART 3: Let-bindings (Scoped Variables)
;; ============================================================================

;; Let-bindings create local variables that don't affect global scope
(display "Using let-binding for local variables...")
(newline)

(let ((local-x 42)
      (local-y 8))
  (begin
    (display "Inside let - local-x: ")
    (display local-x)
    (newline)
    (display "Inside let - local-y: ")
    (display local-y)
    (newline)))

;; local-x and local-y are not accessible here
;; Trying to use them would cause an error

;; ============================================================================
;; PART 4: Let-binding Shadowing
;; ============================================================================

;; Let-bindings can shadow outer variables
(define x 100)

(let ((x 42))
  (begin
    (display "Inside let - x is shadowed to: ")
    (display x)
    (newline)))

(display "Outside let - x is back to: ")
(display x)
(newline)

;; ============================================================================
;; PART 5: Loop Variable Scoping
;; ============================================================================

;; While loops have their own scope
(display "While loop scoping example:")
(newline)

(define counter 0)
(while (< counter 3)
  (begin
    (display "Counter: ")
    (display counter)
    (newline)
    (set! counter (+ counter 1))))

(display "After loop, counter is: ")
(display counter)
(newline)

;; For loops iterate with proper variable scoping
(display "For loop scoping example:")
(newline)

(for item (list "apple" "banana" "cherry")
  (begin
    (display "Item: ")
    (display item)
    (newline)))

;; 'item' is scoped to the loop - not accessible here

;; ============================================================================
;; PART 6: Closures and Captured Variables
;; ============================================================================

;; Closures can capture variables from their defining scope
(define make-counter (lambda (start)
  (lambda ()
    (set! start (+ start 1))
    start)))

(define counter1 (make-counter 10))

(display "Counter1 call 1: ")
(display (counter1))
(newline)
(display "Counter1 call 2: ")
(display (counter1))
(newline)

;; Different closure instance has its own captured value
(define counter2 (make-counter 100))
(display "Counter2 call 1: ")
(display (counter2))
(newline)

;; ============================================================================
;; PART 7: Nested Functions and Scope Chain
;; ============================================================================

;; Inner functions can access outer function's variables
(define outer-var 42)

(define outer-function (lambda (x)
  (define inner-function (lambda (y)
    (+ x y outer-var)))
  (inner-function 10)))

(display "Nested function result: ")
(display (outer-function 5))
(newline)
;; x=5, y=10, outer-var=42 -> 5 + 10 + 42 = 57

;; ============================================================================
;; PART 8: Best Practices
;; ============================================================================

;; 1. Use global variables sparingly
;; 2. Use let-bindings for temporary local variables
;; 3. Use function parameters for values that change behavior
;; 4. Be aware of variable shadowing - it can be confusing!
;; 5. Use descriptive variable names to avoid confusion

;; Example: Good practice - using let for intermediate values
(let ((temp-result (* 5 6))
      (temp-sum (+ 10 20)))
  (display "Good practice result: ")
  (display (+ temp-result temp-sum))
  (newline))

;; ============================================================================
;; PART 9: Common Scoping Mistakes
;; ============================================================================

;; MISTAKE 1: Assuming loop variables persist (they don't in proper scoping)
;; (for i (list 1 2 3) (print i))
;; (print i)  ; ERROR: i is not defined outside the loop

;; MISTAKE 2: Modifying global instead of local
(define count 0)

(lambda ()
  (set! count (+ count 1))  ; Modifies global!
  count)

;; To fix, use a parameter:
(lambda (count)
  (+ count 1))

;; MISTAKE 3: Expecting sequential binding in let (use let* instead)
;; This would fail:
;; (let ((x 5)
;;       (y (+ x 1)))  ; ERROR: x not bound yet in let
;;   (+ x y))

;; ============================================================================
;; END OF SCOPE EXPLANATION
;; ============================================================================

(display "Scope explanation complete!")
(newline)
