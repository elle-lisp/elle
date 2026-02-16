;; Closures, Lambdas, and Recursion in Elle - Comprehensive Guide
;;
;; This example demonstrates:
;; - Lambda expressions and closures
;; - Variable capture and lexical scope
;; - Function composition and chaining
;; - Predicates and boolean functions
;; - List processing with closures
;; - Environment preservation
;; - Nested functions and shadowing
;; - Recursion patterns (self-recursion, mutual recursion, tail recursion)
;; - Mutable state with closures

(import-file "./examples/assertions.lisp")

(display "=== Closures and Lambdas in Elle ===")
(newline)
(newline)

;; ============================================================================
;; SECTION 1: BASIC LAMBDA EXPRESSIONS
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

;; Section 1 Assertions
(assert-eq (identity 42) 42 "identity(42) = 42")
(assert-eq (add 5 3) 8 "add(5, 3) = 8")
(assert-eq (double 21) 42 "double(21) = 42")
(assert-eq (abs-val -15) 15 "abs-val(-15) = 15")
(assert-eq (max-of-two 10 25) 25 "max-of-two(10, 25) = 25")
(newline)

;; ============================================================================
;; SECTION 2: VARIABLE CAPTURE AND LEXICAL SCOPE
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
;; SECTION 3: FUNCTION COMPOSITION AND CHAINING
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

;; Section 3 Assertions
(assert-eq (add-one 5) 6 "add-one(5) = 6")
(assert-eq (times-two 6) 12 "times-two(6) = 12")
(assert-eq after-add-one 6 "composition: 5 + 1 = 6")
(assert-eq after-times-two 12 "composition: (5 + 1) * 2 = 12")
(newline)

;; ============================================================================
;; SECTION 4: PREDICATES AND BOOLEAN FUNCTIONS
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

;; Section 4 Assertions
(assert-eq (is-positive 5) #t "is-positive(5) = true")
(assert-eq (is-negative -3) #t "is-negative(-3) = true")
(assert-eq (is-zero 0) #t "is-zero(0) = true")
(assert-eq (is-even 4) #t "is-even(4) = true")
(assert-eq (is-even 7) #f "is-even(7) = false")
(newline)

;; ============================================================================
;; SECTION 5: LIST PROCESSING WITH CLOSURES
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

;; Section 5 Assertions
(assert-eq (list-sum (list 1 2 3 4 5)) 15 "list-sum([1,2,3,4,5]) = 15")
(assert-eq (list-count (list 1 2 3 4 5)) 5 "list-count([1,2,3,4,5]) = 5")
(assert-eq (find-max (list 3 7 2 9 1 5)) 9 "find-max([3,7,2,9,1,5]) = 9")
(newline)

;; ============================================================================
;; SECTION 6: ENVIRONMENT PRESERVATION - MULTIPLE CLOSURES
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

;; Section 6 Assertions
(assert-eq (closure-1 8) 50 "closure-1(8) = 50")
(assert-eq (closure-2 2) 84 "closure-2(2) = 84")
(assert-eq (above-50 75) #t "above-50(75) = true")
(assert-eq (above-100 75) #f "above-100(75) = false")
(newline)

;; ============================================================================
;; SECTION 7: NESTED FUNCTIONS AND SCOPE SHADOWING
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

;; Section 7 Assertions
(assert-eq (shadow-test 20) 25 "shadow-test(20) = 25")
(assert-eq (use-outer 15) 25 "use-outer(15) = 25")
(newline)

;; ============================================================================
;; SECTION 8: PARAMETER VS. ENVIRONMENT VARIABLES
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

;; Section 8 Assertions
(assert-eq (uses-env 20) 50 "uses-env(20) = 50")
(assert-eq (add-params 10 20 30) 60 "add-params(10,20,30) = 60")
(assert-eq (mixed-func 10) 60 "mixed-func(10) = 60")
(newline)

;; ============================================================================
;; SECTION 9: COMPLEX CONDITIONS IN LAMBDAS
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

;; Section 9 Assertions
(assert-eq (classify -5) "negative" "classify(-5) = negative")
(assert-eq (classify 0) "zero" "classify(0) = zero")
(assert-eq (classify 7) "small" "classify(7) = small")
(assert-eq (classify 50) "medium" "classify(50) = medium")
(assert-eq (classify 200) "large" "classify(200) = large")
(newline)

;; ============================================================================
;; SECTION 10: REUSABLE FUNCTION PATTERNS
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

;; Section 10 Assertions
(assert-eq (make-greeting "Alice") "Hello, Alice!" "make-greeting(Alice) = Hello, Alice!")
(assert-eq (make-greeting "Bob") "Hello, Bob!" "make-greeting(Bob) = Hello, Bob!")
(newline)

;; ============================================================================
;; SECTION 11: LOCAL VARIABLES IN LAMBDA BODIES (PHASE 4)
;; ============================================================================

(display "11. LOCAL VARIABLES IN LAMBDA BODIES (PHASE 4)")
(newline)
(display "--------------------------------")
(newline)

;; Before Phase 4, define inside fn didn't work properly.
;; Now local variables are stored as cells in the closure environment.
(define compute
  (fn ()
    (begin
      (define x 10)
      (define y 20)
      (+ x y))))

(display "Local variables: (compute) = ")
(define result1 (compute))
(display result1)
(newline)
(assert-eq result1 30 "Local variables should compute to 30")
(newline)

;; ============================================================================
;; SECTION 12: MUTATION WITH set! INSIDE LAMBDAS (PHASE 4)
;; ============================================================================

(display "12. MUTATION WITH set! INSIDE LAMBDAS (PHASE 4)")
(newline)
(display "--------------------------------")
(newline)

;; set! now works on locally-defined variables inside fn bodies
(define mutate-test
  (fn ()
    (begin
      (define x 0)
      (set! x 42)
      x)))

(display "set! mutation: (mutate-test) = ")
(define result2 (mutate-test))
(display result2)
(newline)
(assert-eq result2 42 "set! mutation should result in 42")
(newline)

;; ============================================================================
;; SECTION 13: NESTED CLOSURE CAPTURE OF LOCAL VARIABLES (PHASE 4)
;; ============================================================================

(display "13. NESTED CLOSURE CAPTURE OF LOCAL VARIABLES (PHASE 4)")
(newline)
(display "--------------------------------")
(newline)

;; A locally-defined variable can be captured by a nested fn
(define make-adder
  (fn (base)
    (begin
      (define offset 10)
      (fn (x) (+ base offset x)))))

(define add-with-offset (make-adder 100))

(display "Nested capture: (add-with-offset 5) = ")
(define result3 (add-with-offset 5))
(display result3)
(newline)
(assert-eq result3 115 "Nested capture should result in 115")
(newline)

;; ============================================================================
;; SECTION 14: SHARED MUTABLE STATE — COUNTER PATTERN (PHASE 4)
;; ============================================================================

(display "14. SHARED MUTABLE STATE — COUNTER PATTERN (PHASE 4)")
(newline)
(display "--------------------------------")
(newline)

;; The classic closure counter pattern: a factory that returns
;; a closure sharing mutable state via a cell
(define make-counter
  (fn ()
    (begin
      (define count 0)
      (fn ()
        (begin
          (set! count (+ count 1))
          count)))))

(define counter (make-counter))

(display "Counter sequence: ")
(define c1-val1 (counter))
(display c1-val1)
(display " ")
(define c1-val2 (counter))
(display c1-val2)
(display " ")
(define c1-val3 (counter))
(display c1-val3)
(newline)
(assert-eq c1-val1 1 "First counter call should be 1")
(assert-eq c1-val2 2 "Second counter call should be 2")
(assert-eq c1-val3 3 "Third counter call should be 3")
(newline)

;; ============================================================================
;; SECTION 15: INDEPENDENT COUNTERS (SEPARATE STATE) (PHASE 4)
;; ============================================================================

(display "15. INDEPENDENT COUNTERS (SEPARATE STATE) (PHASE 4)")
(newline)
(display "--------------------------------")
(newline)

;; Each call to make-counter creates independent state
(define c1 (make-counter))
(define c2 (make-counter))

(display "c1 sequence: ")
(define c1-a (c1))
(display c1-a)
(display " ")
(define c1-b (c1))
(display c1-b)
(newline)
(assert-eq c1-a 1 "c1 first call should be 1")
(assert-eq c1-b 2 "c1 second call should be 2")

(display "c2 sequence: ")
(define c2-a (c2))
(display c2-a)
(newline)
(assert-eq c2-a 1 "c2 first call should be 1 (independent)")
(newline)

;; ============================================================================
;; SECTION 16: ACCUMULATOR PATTERN (PHASE 4)
;; ============================================================================

(display "16. ACCUMULATOR PATTERN (PHASE 4)")
(newline)
(display "--------------------------------")
(newline)

;; An accumulator that adds values to a running total
(define make-accumulator
  (fn (initial)
    (begin
      (define total initial)
      (fn (amount)
        (begin
          (set! total (+ total amount))
          total)))))

(define acc (make-accumulator 100))

(display "Accumulator sequence: ")
(define acc1 (acc 10))
(display acc1)
(display " ")
(define acc2 (acc 20))
(display acc2)
(display " ")
(define acc3 (acc 5))
(display acc3)
(newline)
(assert-eq acc1 110 "Accumulator first call should be 110")
(assert-eq acc2 130 "Accumulator second call should be 130")
(assert-eq acc3 135 "Accumulator third call should be 135")
(newline)

;; ============================================================================
;; SECTION 17: WHILE LOOP WITH set! (ISSUE #106 FIX) (PHASE 4)
;; ============================================================================

(display "17. WHILE LOOP WITH set! (ISSUE #106 FIX) (PHASE 4)")
(newline)
(display "--------------------------------")
(newline)

;; This was the original bug from issue #106:
;; set! inside a fn body used to fail with "Undefined global variable"
(define sum-to-n
  (fn (n)
    (begin
      (define result 0)
      (define i 1)
      (while (<= i n)
        (begin
          (set! result (+ result i))
          (set! i (+ i 1))))
      result)))

(display "Sum 1..10: ")
(define sum-result (sum-to-n 10))
(display sum-result)
(newline)
(assert-eq sum-result 55 "Sum 1..10 should be 55")
(newline)

;; ============================================================================
;; SECTION 18: MULTIPLE CLOSURES SHARING STATE (PHASE 4)
;; ============================================================================

(display "18. MULTIPLE CLOSURES SHARING STATE (PHASE 4)")
(newline)
(display "--------------------------------")
(newline)

;; A pair of closures that share the same mutable cell
;; (getter and setter pattern)
;; We return multiple closures from one factory using a list.
(define make-box
  (fn (initial)
    (begin
      (define value initial)
      (list
        (fn () value)                          ;; getter
        (fn (new-val) (set! value new-val))))))  ;; setter

(define box (make-box 0))
(define box-get (first box))
(define box-set (first (rest box)))

(display "Box initial: ")
(define box-initial (box-get))
(display box-initial)
(newline)
(assert-eq box-initial 0 "Box initial value should be 0")

(box-set 42)

(display "Box after set: ")
(define box-after (box-get))
(display box-after)
(newline)
(assert-eq box-after 42 "Box after set should be 42")
(newline)

;; ============================================================================
;; SUMMARY
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
(display "Phase 4 enables shared mutable captures via cell boxing:")
(newline)
(display "  • Local variables in fn bodies - define inside lambdas")
(newline)
(display "  • Mutation with set! - modify locally-defined variables")
(newline)
(display "  • Nested closure capture - inner lambdas capture outer locals")
(newline)
(display "  • Shared mutable state - closures share cells, not copies")
(newline)
(display "  • Counter pattern - classic closure-based state machines")
(newline)
(display "  • Accumulator pattern - running totals with mutable state")
(newline)
(display "  • While loops with set! - loops can mutate local variables")
(newline)
(display "  • Getter/setter pairs - multiple closures sharing one cell")
(newline)
(newline)

(display "=== All Examples Complete - All Assertions Passed ===")
(newline)

; ============================================================================
; SECTION 19: RECURSION PATTERNS
; ============================================================================

(display "\n=== RECURSION PATTERNS ===")
(newline)
(newline)

; ============================================================================
; Part 1: Self-Recursion - Fibonacci
; ============================================================================

(display "Part 1: Self-Recursion - Fibonacci")
(newline)
(newline)

;; Define a recursive Fibonacci function
(define fib (lambda (n)
  (if (= n 0)
    0
    (if (= n 1)
      1
      (+ (fib (- n 1)) (fib (- n 2)))))))

;; Display computed Fibonacci numbers
(display "Fibonacci Sequence (Computed):")
(newline)

(display "F(0) = ")
(display (fib 0))
(newline)

(display "F(1) = ")
(display (fib 1))
(newline)

(display "F(2) = ")
(display (fib 2))
(newline)

(display "F(3) = ")
(display (fib 3))
(newline)

(display "F(4) = ")
(display (fib 4))
(newline)

(display "F(5) = ")
(display (fib 5))
(newline)

(display "F(10) = ")
(display (fib 10))
(newline)

;; Verify basic Fibonacci values
(assert-eq (fib 0) 0 "fib(0) = 0")
(assert-eq (fib 1) 1 "fib(1) = 1")
(assert-eq (fib 2) 1 "fib(2) = 1")
(assert-eq (fib 3) 2 "fib(3) = 2")
(assert-eq (fib 4) 3 "fib(4) = 3")
(assert-eq (fib 5) 5 "fib(5) = 5")
(assert-eq (fib 10) 55 "fib(10) = 55")

(newline)

;; More efficient Fibonacci using accumulator/tail-recursion pattern
(display "Self-Recursion with Accumulator (Tail-Recursive):")
(newline)
(newline)

(define fib-acc (lambda (n)
  (define helper (lambda (n acc1 acc2)
    (if (= n 0)
      acc1
      (helper (- n 1) acc2 (+ acc1 acc2)))))
  (helper n 0 1)))

(display "F(10) = ")
(display (fib-acc 10))
(newline)

(display "F(15) = ")
(display (fib-acc 15))
(newline)

(display "F(20) = ")
(display (fib-acc 20))
(newline)

;; Verify accumulator-based Fibonacci
(assert-eq (fib-acc 0) 0 "fib-acc(0) = 0")
(assert-eq (fib-acc 5) 5 "fib-acc(5) = 5")
(assert-eq (fib-acc 10) 55 "fib-acc(10) = 55")
(assert-eq (fib-acc 15) 610 "fib-acc(15) = 610")
(assert-eq (fib-acc 20) 6765 "fib-acc(20) = 6765")

(newline)

;; Factorial - another self-recursive example
(display "Self-Recursion - Factorial:")
(newline)
(newline)

(define factorial (lambda (n)
  (if (= n 0)
    1
    (* n (factorial (- n 1))))))

(display "5! = ")
(display (factorial 5))
(newline)

(assert-eq (factorial 0) 1 "factorial(0) = 1")
(assert-eq (factorial 5) 120 "factorial(5) = 120")
(assert-eq (factorial 6) 720 "factorial(6) = 720")

(newline)

; ============================================================================
; Part 2: Mutual Recursion
; ============================================================================

(display "Part 2: Mutual Recursion - Even/Odd Predicates")
(newline)
(newline)

(define is-even
  (fn (n)
    (if (= n 0)
      #t
      (is-odd (- n 1)))))

(define is-odd
  (fn (n)
    (if (= n 0)
      #f
      (is-even (- n 1)))))

(display "Testing even/odd predicates:")
(newline)
(display "is-even(0): ")
(display (is-even 0))
(newline)
(display "is-even(4): ")
(display (is-even 4))
(newline)
(display "is-even(7): ")
(display (is-even 7))
(newline)
(display "is-odd(1): ")
(display (is-odd 1))
(newline)
(display "is-odd(5): ")
(display (is-odd 5))
(newline)
(display "is-odd(8): ")
(display (is-odd 8))
(newline)

;; Verify mutual recursion
(assert-eq (is-even 0) #t "is-even(0) = #t")
(assert-eq (is-even 4) #t "is-even(4) = #t")
(assert-eq (is-even 7) #f "is-even(7) = #f")
(assert-eq (is-odd 1) #t "is-odd(1) = #t")
(assert-eq (is-odd 5) #t "is-odd(5) = #t")
(assert-eq (is-odd 8) #f "is-odd(8) = #f")

(newline)

; ============================================================================
; Part 3: Recursion with Nested Definitions
; ============================================================================

(display "Part 3: Recursion with Nested Definitions")
(newline)
(newline)

(define run-factorial (fn (n)
  (begin
    (define fact (fn (x) (if (= x 0) 1 (* x (fact (- x 1))))))
    (fact n))))

(display "Factorial of 6 (nested): ")
(display (run-factorial 6))
(newline)

;; Verify nested recursion
(assert-eq (run-factorial 6) 720 "Factorial of 6 should be 720")
(assert-eq (run-factorial 5) 120 "Factorial of 5 should be 120")
(assert-eq (run-factorial 0) 1 "Factorial of 0 should be 1")

(newline)

;; Mutual recursion with nested definitions requires letrec
;; (define does not support forward references)
(define run-even-odd (fn ()
  (letrec ((is-even-local (fn (n) (if (= n 0) #t (is-odd-local (- n 1)))))
           (is-odd-local (fn (n) (if (= n 0) #f (is-even-local (- n 1))))))
    (is-even-local 8))))

(display "Is 8 even (nested letrec)? ")
(display (run-even-odd))
(newline)

;; Verify nested mutual recursion with letrec
(assert-eq (run-even-odd) #t "8 should be even")

(newline)

; ============================================================================
; Part 4: Countdown with Two Functions
; ============================================================================

(display "Part 4: Countdown with Two Functions")
(newline)
(newline)

(define count-down-a
  (fn (n)
    (if (= n 0)
      (display "A: Done!")
      (begin
        (display "A: ")
        (display n)
        (newline)
        (count-down-b (- n 1))))))

(define count-down-b
  (fn (n)
    (if (= n 0)
      (display "B: Done!")
      (begin
        (display "B: ")
        (display n)
        (newline)
        (count-down-a (- n 1))))))

(count-down-a 4)
(newline)
(newline)

; ============================================================================
; Part 5: String Processing with Mutual Recursion
; ============================================================================

(display "Part 5: String Processing with Mutual Recursion")
(newline)
(newline)

(define process-words
  (fn (words)
    (if (= (length words) 0)
      ""
      (string-append
        (string-upcase (first words))
        " "
        (process-separators (rest words))))))

(define process-separators
  (fn (words)
    (if (= (length words) 0)
      ""
      (string-append
        "-"
        (process-words words)))))

(display "Processing: ")
(display (process-words (list "hello" "world" "elle")))
(newline)
(newline)

; ============================================================================
; Part 6: Factorial with Helper - Mutual Style
; ============================================================================

(display "Part 6: Factorial with Helper Function")
(newline)
(newline)

(define factorial-mutual
  (fn (n)
    (factorial-helper-mutual n 1)))

(define factorial-helper-mutual
  (fn (n acc)
    (if (= n 0)
      acc
      (factorial-helper-mutual (- n 1) (* acc n)))))

(display "factorial-mutual(5): ")
(display (factorial-mutual 5))
(newline)

(display "factorial-mutual(7): ")
(display (factorial-mutual 7))
(newline)

;; Verify factorial with helper
(assert-eq (factorial-mutual 5) 120 "factorial-mutual(5) = 120")
(assert-eq (factorial-mutual 7) 5040 "factorial-mutual(7) = 5040")

(newline)

; ============================================================================
; Part 7: Three-Way Mutual Recursion
; ============================================================================

(display "Part 7: Three-Way Mutual Recursion")
(newline)
(newline)

(define func-a
  (fn (n)
    (if (= n 0)
      "A-done"
      (func-b (- n 1)))))

(define func-b
  (fn (n)
    (if (= n 0)
      "B-done"
      (func-c (- n 1)))))

(define func-c
  (fn (n)
    (if (= n 0)
      "C-done"
      (func-a (- n 1)))))

(display "func-a(5): ")
(display (func-a 5))
(newline)

;; Verify three-way recursion
;; func-a(5) -> func-b(4) -> func-c(3) -> func-a(2) -> func-b(1) -> func-c(0) = "C-done"
(assert-eq (func-a 5) "C-done" "func-a(5) = C-done")
(assert-eq (func-b 4) "C-done" "func-b(4) = C-done")
(assert-eq (func-c 3) "C-done" "func-c(3) = C-done")
(assert-eq (func-a 0) "A-done" "func-a(0) = A-done")
(assert-eq (func-b 0) "B-done" "func-b(0) = B-done")
(assert-eq (func-c 0) "C-done" "func-c(0) = C-done")

(newline)

; ============================================================================
; Part 8: Filtering with Mutual Recursion
; ============================================================================

(display "Part 8: Filtering with Mutual Recursion")
(newline)
(newline)

(define separate-numbers
  (fn (nums)
    (separate-helper-nums nums (list) (list))))

(define separate-helper-nums
  (fn (nums evens odds)
    (if (= (length nums) 0)
      (list evens odds)
      (if (= (mod (first nums) 2) 0)
        (separate-helper-nums (rest nums) (append evens (list (first nums))) odds)
        (separate-helper-nums (rest nums) evens (append odds (list (first nums))))))))

(display "Input: (1 2 3 4 5 6)")
(newline)
(define separated (separate-numbers (list 1 2 3 4 5 6)))
(display "Evens: ")
(display (first separated))
(newline)
(display "Odds: ")
(display (first (rest separated)))
(newline)

;; Verify filtering
(assert-list-eq (first separated) (list 2 4 6) "Evens should be (2 4 6)")
(assert-list-eq (first (rest separated)) (list 1 3 5) "Odds should be (1 3 5)")

(newline)

; ============================================================================
; Part 9: Alternating Pattern with Limited Depth
; ============================================================================

(display "Part 9: Alternating Pattern")
(newline)
(newline)

(define step-x
  (fn (n)
    (if (= n 0)
      "X"
      (step-y (- n 1)))))

(define step-y
  (fn (n)
    (if (= n 0)
      "Y"
      (step-x (- n 1)))))

(display "step-x(3): ")
(display (step-x 3))
(newline)

(display "step-y(4): ")
(display (step-y 4))
(newline)

;; Verify alternating pattern
(assert-eq (step-x 3) "Y" "step-x(3) = Y")
(assert-eq (step-y 4) "Y" "step-y(4) = Y")
(assert-eq (step-x 0) "X" "step-x(0) = X")
(assert-eq (step-y 0) "Y" "step-y(0) = Y")

(newline)

(display "=== All Recursion Examples Complete - All Assertions Passed ===")
(newline)
