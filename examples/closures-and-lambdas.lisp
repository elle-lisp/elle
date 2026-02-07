;; Closures and Lambdas - Functional Programming in Elle
;; This example demonstrates real, executable closures and lambdas.
;; All code here actually runs and demonstrates the feature it claims.

(display "=== Closures and Lambdas in Elle ===")
(newline)
(newline)

;; ============================================================================
;; 1. Basic Lambda Expressions - Create Functions Dynamically
;; ============================================================================

(display "1. BASIC LAMBDA EXPRESSIONS")
(newline)
(display "--------------------------------")
(newline)

;; Identity function - returns its argument unchanged
(define identity (lambda (x) x))
(display "Identity function: (identity 42) = ")
(display (identity 42))
(newline)

;; Addition function - adds two numbers
(define add (lambda (x y) (+ x y)))
(display "Addition function: (add 5 3) = ")
(display (add 5 3))
(newline)

;; Doubling function - multiplies by 2
(define double (lambda (x) (* x 2)))
(display "Doubling function: (double 21) = ")
(display (double 21))
(newline)

;; Absolute value function
(define abs-val (lambda (x)
  (if (< x 0) (- x) x)))
(display "Absolute value: (abs-val -15) = ")
(display (abs-val -15))
(newline)

;; Maximum of two values
(define max-of-two (lambda (x y)
  (if (> x y) x y)))
(display "Maximum of two: (max-of-two 10 25) = ")
(display (max-of-two 10 25))
(newline)
(newline)

;; ============================================================================
;; 2. Variable Capture in Closures - Lexical Scope
;; ============================================================================

(display "2. VARIABLE CAPTURE AND LEXICAL SCOPE")
(newline)
(display "--------------------------------")
(newline)

;; Create a closure that captures the outer variable 'base'
(define base 100)
(define add-to-base (lambda (x) (+ base x)))
(display "Closure capturing 'base' (100): (add-to-base 50) = ")
(display (add-to-base 50))
(newline)

;; Multiple variables captured from outer scope
(define multiplier 2)
(define compute (lambda (x) (+ base (* multiplier x))))
(display "Capturing 'base' and 'multiplier': (compute 25) = ")
(display (compute 25))
(newline)

;; Captured value changes persist in closure
(define offset 10)
(define apply-offset (lambda (x) (+ offset x)))
(display "With offset 10: (apply-offset 5) = ")
(display (apply-offset 5))
(newline)
(newline)

;; ============================================================================
;; 3. Function Composition and Chaining
;; ============================================================================

(display "3. FUNCTION COMPOSITION")
(newline)
(display "--------------------------------")
(newline)

;; Create specialized functions that work together
(define add-one (lambda (x) (+ x 1)))
(define times-two (lambda (x) (* x 2)))

;; Apply functions in sequence
(define x-value 5)
(display "Starting value: ")
(display x-value)
(newline)

(define after-add-one (add-one x-value))
(display "After add-one: ")
(display after-add-one)
(newline)

(define after-times-two (times-two after-add-one))
(display "Then times-two: ")
(display after-times-two)
(newline)
(newline)

;; ============================================================================
;; 4. Predicates and Filters - Functions Returning Booleans
;; ============================================================================

(display "4. PREDICATES AND BOOLEAN FUNCTIONS")
(newline)
(display "--------------------------------")
(newline)

;; Create predicates (functions returning boolean)
(define is-positive (lambda (x) (> x 0)))
(define is-negative (lambda (x) (< x 0)))
(define is-zero (lambda (x) (= x 0)))
(define is-even (lambda (x) (= (mod x 2) 0)))

(display "is-positive(5) = ")
(display (is-positive 5))
(newline)

(display "is-negative(-3) = ")
(display (is-negative -3))
(newline)

(display "is-zero(0) = ")
(display (is-zero 0))
(newline)

(display "is-even(4) = ")
(display (is-even 4))
(newline)

(display "is-even(7) = ")
(display (is-even 7))
(newline)
(newline)

;; ============================================================================
;; 5. List Processing with Closures
;; ============================================================================

(display "5. LIST OPERATIONS WITH CLOSURES")
(newline)
(display "--------------------------------")
(newline)

;; List manipulation functions
(define list-sum (lambda (lst)
  (if (= (length lst) 0)
    0
    (+ (first lst) (list-sum (rest lst))))))

(define my-list (list 1 2 3 4 5))
(display "Sum of list: ")
(display (list-sum my-list))
(newline)

;; List length counter
(define list-count (lambda (lst)
  (if (= (length lst) 0)
    0
    (+ 1 (list-count (rest lst))))))

(display "Count of list: ")
(display (list-count my-list))
(newline)

;; List doubler
(define double-all (lambda (lst)
  (if (= (length lst) 0)
    (list)
    (cons (* 2 (first lst)) (double-all (rest lst))))))

(display "Double each element: ")
(display (double-all (list 1 2 3 4 5)))
(newline)

;; Find maximum in list
(define find-max (lambda (lst)
  (if (= (length lst) 1)
    (first lst)
    (max-of-two (first lst) (find-max (rest lst))))))

(display "Maximum of list: ")
(display (find-max (list 3 7 2 9 1 5)))
(newline)
(newline)

;; ============================================================================
;; 6. Environment Preservation - Multiple Closures
;; ============================================================================

(display "6. ENVIRONMENT PRESERVATION")
(newline)
(display "--------------------------------")
(newline)

;; Each closure has its own captured environment
(define outer-base 42)
(define closure-1 (lambda (x) (+ outer-base x)))
(define closure-2 (lambda (x) (* outer-base x)))

(display "Both closures capture outer-base (42)")
(newline)

(display "closure-1 with +: (closure-1 8) = ")
(display (closure-1 8))
(newline)

(display "closure-2 with *: (closure-2 2) = ")
(display (closure-2 2))
(newline)

;; Multiple closures with different captured context
(define threshold-1 50)
(define above-50 (lambda (x) (> x threshold-1)))

(define threshold-2 100)
(define above-100 (lambda (x) (> x threshold-2)))

(display "Is 75 above 50? ")
(display (above-50 75))
(newline)

(display "Is 75 above 100? ")
(display (above-100 75))
(newline)
(newline)

;; ============================================================================
;; 7. Nested Functions and Scope Shadowing
;; ============================================================================

(display "7. NESTED FUNCTIONS AND SHADOWING")
(newline)
(display "--------------------------------")
(newline)

;; Outer and inner scopes
(define outer-x 10)

;; This lambda shadows the outer-x with its parameter
(define shadow-test (lambda (outer-x)
  (+ outer-x 5)))

(display "Outer outer-x: 10")
(newline)

(display "shadow-test(20) with shadowed parameter: ")
(display (shadow-test 20))
(newline)

;; The original outer-x is unchanged
(define use-outer (lambda (y)
  (+ outer-x y)))

(display "use-outer still sees original: (use-outer 15) = ")
(display (use-outer 15))
(newline)
(newline)

;; ============================================================================
;; 8. Parameter vs. Environment Variables
;; ============================================================================

(display "8. PARAMETER VS. ENVIRONMENT VARIABLES")
(newline)
(display "--------------------------------")
(newline)

;; Environment variables are accessed from closure scope
(define env-var 30)
(define uses-env (lambda (x) (+ env-var x)))
(display "Environment variable capture: (uses-env 20) = ")
(display (uses-env 20))
(newline)

;; Parameters are local to the function
(define add-params (lambda (a b c)
  (+ a b c)))
(display "Function parameters: (add-params 10 20 30) = ")
(display (add-params 10 20 30))
(newline)

;; Both parameters and environment variables work together
(define mixed-context 50)
(define mixed-func (lambda (param)
  (+ mixed-context param)))
(display "Mixed (env + param): (mixed-func 10) = ")
(display (mixed-func 10))
(newline)
(newline)

;; ============================================================================
;; 9. Complex Conditions in Lambdas
;; ============================================================================

(display "9. CONDITIONAL LOGIC IN CLOSURES")
(newline)
(display "--------------------------------")
(newline)

;; Complex conditional logic
(define classify (lambda (n)
  (if (< n 0)
    "negative"
    (if (= n 0)
      "zero"
      (if (< n 10)
        "small"
        (if (< n 100)
          "medium"
          "large"))))))

(display "Classify -5: ")
(display (classify -5))
(newline)

(display "Classify 0: ")
(display (classify 0))
(newline)

(display "Classify 7: ")
(display (classify 7))
(newline)

(display "Classify 50: ")
(display (classify 50))
(newline)

(display "Classify 200: ")
(display (classify 200))
(newline)
(newline)

;; ============================================================================
;; 10. Reusable Function Patterns
;; ============================================================================

(display "10. REUSABLE FUNCTION PATTERNS")
(newline)
(display "--------------------------------")
(newline)

;; Template pattern - parameterized function behavior
(define threshold 10)
(define filter-above (lambda (lst)
  (if (= (length lst) 0)
    (list)
    (if (> (first lst) threshold)
      (cons (first lst) (filter-above (rest lst)))
      (filter-above (rest lst))))))

(display "Filter values above 10: ")
(display (filter-above (list 5 15 8 20 3 12)))
(newline)

;; String builder with captured parts
(define greeting-start "Hello")
(define greeting-end "!")
(define make-greeting (lambda (name)
  (string-append greeting-start ", " name greeting-end)))

(display "Greeting: ")
(display (make-greeting "Alice"))
(newline)

(display "Greeting: ")
(display (make-greeting "Bob"))
(newline)
(newline)

;; ============================================================================
;; 11. Summary
;; ============================================================================

(display "=== SUMMARY ===")
(newline)
(display "Closures and lambdas enable:")
(newline)
(display "  • Anonymous functions - define functions without names")
(newline)
(display "  • Variable capture - functions access outer scope variables")
(newline)
(display "  • Lexical scoping - inner functions see outer definitions")
(newline)
(display "  • Parameter shadowing - local params override outer values")
(newline)
(display "  • List processing - filter, map, and transform sequences")
(newline)
(display "  • Predicates - functions returning boolean values")
(newline)
(display "  • Complex logic - conditionals and pattern matching")
(newline)
(display "  • Reusable patterns - template functions for common tasks")
(newline)
(newline)

(display "=== All Examples Complete ===")
(newline)
