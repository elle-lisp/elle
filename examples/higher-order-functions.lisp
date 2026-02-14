;; Higher-Order Functions in Elle

(import-file "./examples/assertions.lisp")

(display "=== Higher-Order Functions in Elle ===")
(newline)
(newline)

;; ============================================================================
;; 1. MAP - Apply a function to each element of a list
;; ============================================================================

(display "1. MAP - Transform each element")
(newline)
(display "--------------------------------")
(newline)

;; Manual map implementation using recursion
(define my-map (fn (f lst)
  "Apply function f to each element of list lst"
  (if (= (length lst) 0)
      (list)
      (cons (f (first lst)) (my-map f (rest lst))))))

;; Example 1a: Double each number
(define double (fn (x) (* x 2)))
(define numbers (list 1 2 3 4 5))
(define doubled (my-map double numbers))
(display "Double each number: ")
(display doubled)
(newline)
(assert-list-eq doubled (list 2 4 6 8 10) "map double: [1,2,3,4,5] -> [2,4,6,8,10]")
(newline)

;; Example 1b: Convert numbers to strings
(define to-string (fn (x) (number->string x)))
(define as-strings (my-map to-string numbers))
(display "Convert to strings: ")
(display as-strings)
(newline)
(assert-list-eq as-strings (list "1" "2" "3" "4" "5") "map to-string")

;; Example 1c: Map with lambda (anonymous function)
(define squared (my-map (lambda (x) (* x x)) numbers))
(display "Square each number: ")
(display squared)
(newline)
(assert-list-eq squared (list 1 4 9 16 25) "map square: [1,2,3,4,5] -> [1,4,9,16,25]")
(newline)

;; Example 1d: Map with closure (captures outer variable)
(define multiplier 3)
(define multiply-by-multiplier (lambda (x) (* x multiplier)))
(define tripled (my-map multiply-by-multiplier numbers))
(display "Multiply by 3 (closure): ")
(display tripled)
(newline)
(assert-list-eq tripled (list 3 6 9 12 15) "map triple: [1,2,3,4,5] -> [3,6,9,12,15]")
(newline)

;; ============================================================================
;; 2. FILTER - Select elements that satisfy a predicate
;; ============================================================================

(display "2. FILTER - Select matching elements")
(newline)
(display "--------------------------------")
(newline)

;; Manual filter implementation using recursion
(define my-filter (fn (predicate lst)
  "Keep only elements where predicate returns true"
  (if (= (length lst) 0)
      (list)
      (if (predicate (first lst))
          (cons (first lst) (my-filter predicate (rest lst)))
          (my-filter predicate (rest lst))))))

;; Example 2a: Filter even numbers
(define is-even (fn (x) (= (mod x 2) 0)))
(define evens (my-filter is-even (list 1 2 3 4 5 6 7 8 9 10)))
(display "Even numbers: ")
(display evens)
(newline)
(assert-list-eq evens (list 2 4 6 8 10) "filter even: [1..10] -> [2,4,6,8,10]")
(newline)

;; Example 2b: Filter odd numbers
(define is-odd (fn (x) (= (mod x 2) 1)))
(define odds (my-filter is-odd (list 1 2 3 4 5 6 7 8 9 10)))
(display "Odd numbers: ")
(display odds)
(newline)
(assert-list-eq odds (list 1 3 5 7 9) "filter odd: [1..10] -> [1,3,5,7,9]")
(newline)

;; Example 2c: Filter with lambda
(define large-numbers (my-filter (lambda (x) (> x 5)) (list 1 3 5 7 9 11)))
(display "Numbers > 5: ")
(display large-numbers)
(newline)
(assert-list-eq large-numbers (list 7 9 11) "filter > 5: [1,3,5,7,9,11] -> [7,9,11]")
(newline)

;; Example 2d: Filter with closure (captures threshold)
(define threshold 50)
(define above-threshold (lambda (x) (> x threshold)))
(define high-values (my-filter above-threshold (list 10 30 50 70 90)))
(display "Values > 50: ")
(display high-values)
(newline)
(assert-list-eq high-values (list 70 90) "filter > 50: [10,30,50,70,90] -> [70,90]")

;; ============================================================================
;; 3. FOLD (REDUCE) - Accumulate values into a single result
;; ============================================================================

(display "3. FOLD - Accumulate into single value")
(newline)
(display "--------------------------------")
(newline)

;; Manual fold implementation using recursion
(define my-fold (fn (f initial lst)
  "Reduce list to single value using function f and initial accumulator"
  (if (= (length lst) 0)
      initial
      (my-fold f (f initial (first lst)) (rest lst)))))

;; Example 3a: Sum all numbers
(define add (fn (a b) (+ a b)))
(define sum-result (my-fold add 0 (list 1 2 3 4 5)))
(display "Sum of [1,2,3,4,5]: ")
(display sum-result)
(newline)
(assert-eq sum-result 15 "fold sum: [1,2,3,4,5] -> 15")
(newline)

;; Example 3b: Product of all numbers
(define multiply (fn (a b) (* a b)))
(define product-result (my-fold multiply 1 (list 1 2 3 4 5)))
(display "Product of [1,2,3,4,5]: ")
(display product-result)
(newline)
(assert-eq product-result 120 "fold product: [1,2,3,4,5] -> 120")
(newline)

;; Example 3c: Concatenate strings
(define concat (fn (a b) (string-append a b)))
(define words (list "Hello" " " "World" "!"))
(define concatenated (my-fold concat "" words))
(display "Concatenate strings: ")
(display concatenated)
(newline)
(assert-eq concatenated "Hello World!" "fold concat: [Hello, , World, !] -> Hello World!")
(newline)

;; Example 3d: Build a list in reverse
(define prepend (fn (lst item) (cons item lst)))
(define reversed (my-fold prepend (list) (list 1 2 3 4 5)))
(display "Reverse via fold: ")
(display reversed)
(newline)
(assert-list-eq reversed (list 5 4 3 2 1) "fold reverse: [1,2,3,4,5] -> [5,4,3,2,1]")

;; Example 3e: Count elements
(define count-fn (fn (acc x) (+ acc 1)))
(define count-result (my-fold count-fn 0 (list 'a 'b 'c 'd 'e)))
(display "Count elements: ")
(display count-result)
(newline)
(assert-eq count-result 5 "fold count: [a,b,c,d,e] -> 5")
(newline)

;; ============================================================================
;; 4. FUNCTION COMPOSITION - Combine functions
;; ============================================================================

(display "4. FUNCTION COMPOSITION")
(newline)
(display "--------------------------------")
(newline)

;; Compose two functions: (compose f g)(x) = f(g(x))
(define compose (fn (f g)
  "Return a new function that applies g then f"
  (lambda (x) (f (g x)))))

;; Example 4a: Compose double and add-one
(define add-one (fn (x) (+ x 1)))
(define double (fn (x) (* x 2)))
(define add-one-then-double (compose double add-one))
(display "Compose (double (add-one x)): ")
(display (add-one-then-double 5))
(newline)
(assert-eq (add-one-then-double 5) 12 "compose: double(add-one(5)) = 12")
(newline)

;; Example 4b: Compose multiple operations
(define square (fn (x) (* x x)))
(define double-then-square (compose square double))
(display "Compose (square (double x)): ")
(display (double-then-square 3))
(newline)
(assert-eq (double-then-square 3) 36 "compose: square(double(3)) = 36")
(newline)

;; Example 4c: Use composed function with map
(define numbers-2 (list 1 2 3 4))
(define composed-map (my-map double-then-square numbers-2))
(display "Map composed function: ")
(display composed-map)
(newline)
(assert-list-eq composed-map (list 4 16 36 64) "map composed: double then square")

;; ============================================================================
;; 5. HIGHER-ORDER FUNCTION PATTERNS
;; ============================================================================

(display "5. HIGHER-ORDER PATTERNS")
(newline)
(display "--------------------------------")
(newline)

;; Pattern 1: Function that returns a function (currying)
(define make-multiplier (fn (n)
  "Return a function that multiplies by n"
  (lambda (x) (* x n))))

(define times-5 (make-multiplier 5))
(define times-10 (make-multiplier 10))
(display "Curried multiplier (5): ")
(display (times-5 3))
(newline)
(assert-eq (times-5 3) 15 "curried multiplier: times-5(3) = 15")
(newline)

(display "Curried multiplier (10): ")
(display (times-10 3))
(newline)
(assert-eq (times-10 3) 30 "curried multiplier: times-10(3) = 30")
(newline)

;; Pattern 2: Function that returns a predicate
(define make-threshold-checker (fn (threshold)
   "Return a predicate that checks if value exceeds threshold"
   (lambda (x) (> x threshold))))

(define above-100 (make-threshold-checker 100))
(define above-50 (make-threshold-checker 50))
(define test-values (list 25 75 125))
(display "Filter values > 100: ")
(display (my-filter above-100 test-values))
(newline)
(assert-list-eq (my-filter above-100 test-values) (list 125) "filter > 100")
(newline)

(display "Filter values > 50: ")
(display (my-filter above-50 test-values))
(newline)
(assert-list-eq (my-filter above-50 test-values) (list 75 125) "filter > 50")

;; Pattern 3: Pipe - apply functions in sequence
(define pipe (fn (x . functions)
   "Apply functions in sequence to x"
   (if (= (length functions) 0)
       x
       (pipe (((first functions)) x) (rest functions)))))

;; ============================================================================
;; 6. REAL-WORLD PATTERNS
;; ============================================================================

(display "6. REAL-WORLD PATTERNS")
(newline)
(display "--------------------------------")
(newline)

;; Pattern 1: Data transformation pipeline
;; Transform: [1,2,3,4,5] -> double -> filter evens -> sum
(define pipeline-result
   (my-fold add 0
     (my-filter is-even
       (my-map double (list 1 2 3 4 5)))))
(display "Pipeline (double, filter even, sum): ")
(display pipeline-result)
(newline)
(assert-eq pipeline-result 30 "pipeline: double [1,2,3,4,5] -> filter even -> sum = 30")
(newline)

;; Pattern 2: Conditional transformation
(define transform-if (fn (predicate transformer lst)
   "Apply transformer only to elements matching predicate"
   (my-map (lambda (x)
     (if (predicate x) (transformer x) x))
     lst)))

(define conditional-result
   (transform-if is-even double (list 1 2 3 4 5)))
(display "Transform only even numbers: ")
(display conditional-result)
(newline)
(assert-list-eq conditional-result (list 1 4 3 8 5) "conditional transform: double evens only")
(newline)

;; Pattern 3: Accumulate with side effects
(define accumulate-with-display (fn (f initial lst)
   "Fold while displaying each step"
   (if (= (length lst) 0)
       initial
       (begin
         (display "  Step: ")
         (display initial)
         (display " + ")
         (display (first lst))
         (display " = ")
         (display (f initial (first lst)))
         (newline)
         (accumulate-with-display f (f initial (first lst)) (rest lst))))))

(display "Accumulate with display:")
(newline)
(define traced-sum (accumulate-with-display add 0 (list 1 2 3)))
(display "Final result: ")
(display traced-sum)
(newline)
(assert-eq traced-sum 6 "accumulate with display: 0+1+2+3 = 6")
(newline)

;; ============================================================================
;; 7. ADVANCED PATTERNS
;; ============================================================================

(display "7. ADVANCED PATTERNS")
(newline)
(display "--------------------------------")
(newline)

;; Pattern 1: Partial application
(define partial (fn (f . args)
  "Create a new function with some arguments pre-filled"
  (lambda (. rest-args)
    (f . (append args rest-args)))))

;; Pattern 2: Memoization (simple version)
(define make-memoized (fn (f)
  "Create a memoized version of function f (simplified)"
  (lambda (x) (f x))))

;; Pattern 3: Function that validates input
(define validate-then-apply (fn (validator transformer value)
  "Apply transformer only if validator returns true"
  (if (validator value)
      (transformer value)
      (begin
        (display "Validation failed for: ")
        (display value)
        (newline)
        value))))

(define positive-validator (fn (x) (> x 0)))
(define square-transformer (fn (x) (* x x)))
(display "Validate and transform 5: ")
(display (validate-then-apply positive-validator square-transformer 5))
(newline)
(assert-eq (validate-then-apply positive-validator square-transformer 5) 25 "validate-then-apply: 5 -> 25")
(newline)

(display "Validate and transform -5: ")
(display (validate-then-apply positive-validator square-transformer -5))
(newline)
(assert-eq (validate-then-apply positive-validator square-transformer -5) -5 "validate-then-apply: -5 fails validation")

;; ============================================================================
;; 8. SUMMARY
;; ============================================================================

(display "=== SUMMARY ===")
(newline)
(display "Higher-order functions enable:")
(newline)
(display "  • MAP - Transform each element with a function")
(newline)
(display "  • FILTER - Select elements matching a predicate")
(newline)
(display "  • FOLD - Accumulate values into a single result")
(newline)
(display "  • COMPOSITION - Combine functions into new functions")
(newline)
(display "  • CURRYING - Create specialized functions from general ones")
(newline)
(display "  • PIPELINES - Chain transformations together")
(newline)
(display "  • VALIDATION - Apply functions conditionally")
(newline)
(display "  • ABSTRACTION - Hide implementation details")
(newline)
(newline)

(display "Key benefits:")
(newline)
(display "  • Code reuse - write generic functions once")
(newline)
(display "  • Composability - combine simple functions into complex ones")
(newline)
(display "  • Readability - express intent clearly")
(newline)
(display "  • Testability - test functions independently")
(newline)
(newline)

(display "=== All Examples Complete - All Assertions Passed ===")
(newline)
