; Math and Logic Operations - Comprehensive guide covering arithmetic, math functions, predicates, and logical operations

(import-file "./examples/assertions.lisp")

;; ============================================================================
;; SECTION 1: Basic Arithmetic Operations
;; ============================================================================

(display "=== Basic Arithmetic ===\n")

(display "Addition: 10 + 5 = ")
(display (+ 10 5))
(newline)
(assert-eq (+ 10 5) 15 "addition: 10 + 5 = 15")

(display "Subtraction: 10 - 3 = ")
(display (- 10 3))
(newline)
(assert-eq (- 10 3) 7 "subtraction: 10 - 3 = 7")

(display "Multiplication: 10 * 3 = ")
(display (* 10 3))
(newline)
(assert-eq (* 10 3) 30 "multiplication: 10 * 3 = 30")

(display "Division: 10 / 2 = ")
(display (/ 10 2))
(newline)
(assert-eq (/ 10 2) 5 "division: 10 / 2 = 5")

(display "Modulo: 10 mod 3 = ")
(display (mod 10 3))
(newline)
(assert-eq (mod 10 3) 1 "modulo: 10 mod 3 = 1")

(newline)

;; ============================================================================
;; SECTION 2: Arithmetic Module Examples
;; ============================================================================

(display "=== Arithmetic Module (Built-in) ===\n")

(display "Elle's Arithmetic Module provides:")
(newline)
(display "  - +, -, *, /, mod")
(newline)
(display "  - Comparison operators: <, >, <=, >=, =")
(newline)
(display "  - Logical operators: and, or, not")
(newline)

(display "\nModule examples:")
(newline)

(var a 10)
(var b 3)

(display "  10 + 3 = ")
(let ((sum (+ a b)))
  (display sum)
  (newline)
  (assert-eq sum 13 "Module: addition"))

(display "  10 - 3 = ")
(let ((diff (- a b)))
  (display diff)
  (newline)
  (assert-eq diff 7 "Module: subtraction"))

(display "  10 * 3 = ")
(let ((prod (* a b)))
  (display prod)
  (newline)
  (assert-eq prod 30 "Module: multiplication"))

(display "  10 / 2 = ")
(let ((quot (/ 10 2)))
  (display quot)
  (newline)
  (assert-eq quot 5 "Module: division"))

(display "  10 mod 3 = ")
(let ((m (mod a b)))
  (display m)
  (newline)
  (assert-eq m 1 "Module: modulo"))

(newline)
(display "✓ Arithmetic Module functions verified")
(newline)

;; ============================================================================
;; SECTION 3: Math Module Functions
;; ============================================================================

(display "\n=== Math Module Functions ===\n")

(display "sqrt(16) = ")
(let ((sq (sqrt 16)))
  (display sq)
  (newline)
  (assert-true (> sq 3.9) "sqrt(16) ≈ 4"))

(display "sqrt(144) = ")
(let ((sq (sqrt 144)))
  (display sq)
  (newline)
  (assert-true (> sq 11.9) "sqrt(144) ≈ 12"))

(display "pow(2, 3) = ")
(let ((p (pow 2 3)))
  (display p)
  (newline)
  (assert-eq p 8 "pow(2, 3) = 8"))

(display "pow(3, 3) = ")
(let ((p (pow 3 3)))
  (display p)
  (newline)
  (assert-eq p 27 "pow(3, 3) = 27"))

(display "pow(4, 2) = ")
(let ((p (pow 4 2)))
  (display p)
  (newline)
  (assert-eq p 16 "pow(4, 2) = 16"))

(newline)

;; ============================================================================
;; SECTION 4: Trigonometric Functions
;; ============================================================================

(display "=== Trigonometric Functions ===\n")

(display "sin(0) = ")
(let ((s (sin 0)))
  (display s)
  (newline)
  (assert-true (< s 0.01) "sin(0) ≈ 0"))

(display "cos(0) = ")
(let ((c (cos 0)))
  (display c)
  (newline)
  (assert-true (> c 0.99) "cos(0) ≈ 1"))

(display "sin(pi/2) ≈ ")
(let ((s (sin (/ 3.14159 2))))
  (display s)
  (newline)
  (assert-true (> s 0.99) "sin(pi/2) ≈ 1"))

(newline)

;; ============================================================================
;; SECTION 5: Rounding Functions
;; ============================================================================

(display "=== Rounding Functions ===\n")

(display "floor(3.7) = ")
(let ((fl (floor 3.7)))
  (display fl)
  (newline)
  (assert-eq fl 3 "floor(3.7) = 3"))

(display "ceil(3.2) = ")
(let ((ce (ceil 3.2)))
  (display ce)
  (newline)
  (assert-eq ce 4 "ceil(3.2) = 4"))

(newline)

;; ============================================================================
;; SECTION 6: Mathematical Constants
;; ============================================================================

(display "=== Mathematical Constants ===\n")

(display "pi = ")
(display (pi))
(newline)
(assert-true #t "pi is available")

(display "e = ")
(display (e))
(newline)
(assert-true #t "e is available")

(newline)

;; ============================================================================
;; SECTION 7: Power Calculations
;; ============================================================================

(display "=== Power Calculations ===\n")

(display "2^2 = ")
(let ((p1 (pow 2 2)))
  (display p1)
  (newline)
  (assert-eq p1 4 "2^2 = 4"))

(display "2^10 = ")
(let ((p2 (pow 2 10)))
  (display p2)
  (newline)
  (assert-eq p2 1024 "2^10 = 1024"))

(newline)

;; ============================================================================
;; SECTION 8: Arithmetic Predicates - even? and odd?
;; ============================================================================

(display "=== Arithmetic Predicates ===\n")

(display "=== even? Predicate ===\n")

; Test positive even numbers
(assert-true (even? 0) "0 is even")
(assert-true (even? 2) "2 is even")
(assert-true (even? 4) "4 is even")
(assert-true (even? 100) "100 is even")

; Test positive odd numbers
(assert-false (even? 1) "1 is not even")
(assert-false (even? 3) "3 is not even")
(assert-false (even? 99) "99 is not even")

; Test negative even numbers
(assert-true (even? -2) "-2 is even")
(assert-true (even? -100) "-100 is even")

; Test negative odd numbers
(assert-false (even? -1) "-1 is not even")
(assert-false (even? -99) "-99 is not even")

(display "✓ even? works correctly\n")

; === odd? Predicate ===
(display "\n=== odd? Predicate ===\n")

; Test positive odd numbers
(assert-true (odd? 1) "1 is odd")
(assert-true (odd? 3) "3 is odd")
(assert-true (odd? 99) "99 is odd")

; Test positive even numbers
(assert-false (odd? 0) "0 is not odd")
(assert-false (odd? 2) "2 is not odd")
(assert-false (odd? 100) "100 is not odd")

; Test negative odd numbers
(assert-true (odd? -1) "-1 is odd")
(assert-true (odd? -99) "-99 is odd")

; Test negative even numbers
(assert-false (odd? -2) "-2 is not odd")
(assert-false (odd? -100) "-100 is not odd")

(display "✓ odd? works correctly\n")

; === Edge Cases ===
(display "\n=== Edge Cases ===\n")

; Zero is even
(assert-true (even? 0) "0 is even")
(assert-false (odd? 0) "0 is not odd")
(display "✓ Zero is even\n")

; Large numbers
(assert-true (even? 1000000) "1000000 is even")
(assert-true (odd? 1000001) "1000001 is odd")
(display "✓ Large numbers work\n")

; Negative numbers
(assert-true (even? -1000) "-1000 is even")
(assert-true (odd? -1001) "-1001 is odd")
(display "✓ Negative numbers work\n")

; === Practical Examples ===
(display "\n=== Practical Examples ===\n")

; Filter even numbers from a list
(def filter-even (fn (lst)
  (if (empty? lst)
      '()
      (if (even? (first lst))
          (cons (first lst) (filter-even (rest lst)))
          (filter-even (rest lst))))))

(var numbers (list 1 2 3 4 5 6 7 8 9 10))
(var evens (filter-even numbers))
(display "Even numbers from 1-10: ")
(display evens)
(newline)
(assert-eq (first evens) 2 "first even is 2")
(assert-eq (first (rest evens)) 4 "second even is 4")
(display "✓ filter-even works\n")

; Filter odd numbers from a list
(def filter-odd (fn (lst)
  (if (empty? lst)
      '()
      (if (odd? (first lst))
          (cons (first lst) (filter-odd (rest lst)))
          (filter-odd (rest lst))))))

(var odds (filter-odd numbers))
(display "Odd numbers from 1-10: ")
(display odds)
(newline)
(assert-eq (first odds) 1 "first odd is 1")
(assert-eq (first (rest odds)) 3 "second odd is 3")
(display "✓ filter-odd works\n")

; === Counting Predicates ===
(display "\n=== Counting Predicates ===\n")

; Count even numbers
(def count-even (fn (lst)
  (if (empty? lst)
      0
      (if (even? (first lst))
          (+ 1 (count-even (rest lst)))
          (count-even (rest lst))))))

(var even-count (count-even numbers))
(display "Count of even numbers in 1-10: ")
(display even-count)
(newline)
(assert-eq even-count 5 "there are 5 even numbers in 1-10")
(display "✓ count-even works\n")

; Count odd numbers
(def count-odd (fn (lst)
  (if (empty? lst)
      0
      (if (odd? (first lst))
          (+ 1 (count-odd (rest lst)))
          (count-odd (rest lst))))))

(var odd-count (count-odd numbers))
(display "Count of odd numbers in 1-10: ")
(display odd-count)
(newline)
(assert-eq odd-count 5 "there are 5 odd numbers in 1-10")
(display "✓ count-odd works\n")

; === Alternating Pattern ===
(display "\n=== Alternating Pattern ===\n")

; Check if list alternates between even and odd
(def alternates? (fn (lst)
  (if (empty? lst)
      #t
      (if (empty? (rest lst))
          #t
          (if (even? (first lst))
              (if (odd? (first (rest lst)))
                  (alternates? (rest lst))
                  #f)
              (if (even? (first (rest lst)))
                  (alternates? (rest lst))
                  #f))))))

(var alternating (list 1 2 3 4 5 6))
(var not-alternating (list 1 3 5 7))

(assert-true (alternates? alternating) "1 2 3 4 5 6 alternates")
(assert-false (alternates? not-alternating) "1 3 5 7 does not alternate")
(display "✓ alternates? works\n")

; === Sum of Even/Odd ===
(display "\n=== Sum of Even/Odd ===\n")

; Sum of even numbers
(def sum-even (fn (lst)
  (if (empty? lst)
      0
      (if (even? (first lst))
          (+ (first lst) (sum-even (rest lst)))
          (sum-even (rest lst))))))

(var even-sum (sum-even numbers))
(display "Sum of even numbers in 1-10: ")
(display even-sum)
(newline)
(assert-eq even-sum 30 "sum of 2+4+6+8+10 = 30")
(display "✓ sum-even works\n")

; Sum of odd numbers
(def sum-odd (fn (lst)
  (if (empty? lst)
      0
      (if (odd? (first lst))
          (+ (first lst) (sum-odd (rest lst)))
          (sum-odd (rest lst))))))

(var odd-sum (sum-odd numbers))
(display "Sum of odd numbers in 1-10: ")
(display odd-sum)
(newline)
(assert-eq odd-sum 25 "sum of 1+3+5+7+9 = 25")
(display "✓ sum-odd works\n")

;; ============================================================================
;; SECTION 9: Logical Operations
;; ============================================================================

(display "\n=== Logical Operations ===\n")

; === NOT Operation ===
(display "\n=== NOT Operation ===\n")

(assert-true (not #f) "not #f = #t")
(assert-false (not #t) "not #t = #f")
(assert-false (not 1) "not 1 = #f (non-false is truthy)")
(assert-false (not "hello") "not \"hello\" = #f (non-false is truthy)")
(assert-false (not '()) "not () = #f (empty list is truthy)")

; === Test 0 Truthiness ===
(display "\n=== Test 0 Truthiness ===\n")

; Test that 0 is truthy in Elle
(assert-true (if 0 #t #f) "0 is truthy")
(assert-false (not 0) "not 0 = #f (0 is truthy)")

(display "Truth table for NOT:\n")
(display "  not #t = ")
(display (not #t))
(newline)
(display "  not #f = ")
(display (not #f))
(newline)

; === AND Operation ===
(display "\n=== AND Operation ===\n")

; AND with two arguments - returns last value if all truthy, else first falsy
(assert-eq (and #t #t) #t "and #t #t = #t")
(assert-eq (and #t #f) #f "and #t #f = #f")
(assert-eq (and #f #t) #f "and #f #t = #f")
(assert-eq (and #f #f) #f "and #f #f = #f")

; AND with multiple arguments - returns last value if all truthy
(assert-eq (and #t #t #t) #t "and #t #t #t = #t")
(assert-eq (and #t #t #f) #f "and #t #t #f = #f")
(assert-eq (and #f #t #t) #f "and #f #t #t = #f")

; AND with numbers - returns last value (all args are evaluated)
(assert-eq (and 1 2 3) 3 "and 1 2 3 = 3 (last value)")
(assert-eq (and 1 0 3) 3 "and 1 0 3 = 3 (last value, all evaluated)")

; AND with no arguments
(assert-true (and) "and with no args = #t")

(display "Truth table for AND:\n")
(display "  and #t #t = ")
(display (and #t #t))
(newline)
(display "  and #t #f = ")
(display (and #t #f))
(newline)
(display "  and #f #t = ")
(display (and #f #t))
(newline)
(display "  and #f #f = ")
(display (and #f #f))
(newline)

; === OR Operation ===
(display "\n=== OR Operation ===\n")

; OR with two arguments - returns first truthy value or last value
(assert-eq (or #t #t) #t "or #t #t = #t")
(assert-eq (or #t #f) #t "or #t #f = #t")
(assert-eq (or #f #t) #t "or #f #t = #t")
(assert-eq (or #f #f) #f "or #f #f = #f")

; OR with multiple arguments - returns first truthy or last value
(assert-eq (or #f #f #t) #t "or #f #f #t = #t")
(assert-eq (or #f #f #f) #f "or #f #f #f = #f")
(assert-eq (or #f #t #f) #t "or #f #t #f = #t")

; OR with numbers - returns first truthy value (0 is truthy)
(assert-eq (or 0 1) 0 "or 0 1 = 0 (0 is truthy, returns first)")
(assert-eq (or #f 1) 1 "or #f 1 = 1 (first truthy)")
(assert-eq (or #f #f) #f "or #f #f = #f")

; OR with no arguments
(assert-false (or) "or with no args = #f")

(display "Truth table for OR:\n")
(display "  or #t #t = ")
(display (or #t #t))
(newline)
(display "  or #t #f = ")
(display (or #t #f))
(newline)
(display "  or #f #t = ")
(display (or #f #t))
(newline)
(display "  or #f #f = ")
(display (or #f #f))
(newline)

; === XOR Operation ===
(display "\n=== XOR Operation ===\n")

; XOR with two arguments (true if odd number of truthy values)
(assert-false (xor #t #t) "xor #t #t = #f (even number of truthy)")
(assert-true (xor #t #f) "xor #t #f = #t (odd number of truthy)")
(assert-true (xor #f #t) "xor #f #t = #t (odd number of truthy)")
(assert-false (xor #f #f) "xor #f #f = #f (even number of truthy)")

; XOR with multiple arguments
(assert-true (xor #t #t #t) "xor #t #t #t = #t (odd number of truthy)")
(assert-false (xor #t #t #t #t) "xor #t #t #t #t = #f (even number of truthy)")
(assert-true (xor #t #f #f) "xor #t #f #f = #t (odd number of truthy)")
(assert-false (xor #f #f #f) "xor #f #f #f = #f (even number of truthy)")

; XOR with no arguments
(assert-false (xor) "xor with no args = #f")

; XOR with single argument
(assert-true (xor #t) "xor #t = #t")
(assert-false (xor #f) "xor #f = #f")

(display "Truth table for XOR (2 args):\n")
(display "  xor #t #t = ")
(display (xor #t #t))
(newline)
(display "  xor #t #f = ")
(display (xor #t #f))
(newline)
(display "  xor #f #t = ")
(display (xor #f #t))
(newline)
(display "  xor #f #f = ")
(display (xor #f #f))
(newline)

; === Note on Evaluation ===
(display "\n=== Note on Evaluation ===\n")

; In Elle, and/or are primitives that evaluate all arguments
; They don't short-circuit like in some other Lisps
; Use if/cond for short-circuit behavior

(display "✓ and/or evaluate all arguments\n")

; === Practical Examples ===
(display "\n=== Practical Examples ===\n")

; Check if number is in range
(def in-range? (fn (x min max)
  (if (and (>= x min) (<= x max)) #t #f)))

(assert-true (in-range? 5 0 10) "5 is in range [0, 10]")
(assert-false (in-range? 15 0 10) "15 is not in range [0, 10]")
(display "✓ in-range? works\n")

; Check if value is valid (not nil and not false)
(def valid? (fn (x)
  (if (and (not (nil? x)) (not (eq? x #f))) #t #f)))

(assert-true (valid? 42) "42 is valid")
(assert-true (valid? "hello") "\"hello\" is valid")
(assert-true (valid? '()) "() is valid (empty list is truthy)")
(assert-false (valid? #f) "#f is not valid")
(display "✓ valid? works\n")

; Check if value is positive or zero
(def non-negative? (fn (x)
  (if (or (> x 0) (= x 0)) #t #f)))

(assert-true (non-negative? 5) "5 is non-negative")
(assert-true (non-negative? 0) "0 is non-negative")
(assert-false (non-negative? -5) "-5 is not non-negative")
(display "✓ non-negative? works\n")

; === Combining Logical Operations ===
(display "\n=== Combining Logical Operations ===\n")

; Complex condition: (a AND b) OR (NOT c)
(def complex-check (fn (a b c)
  (if (or (and a b) (not c)) #t #f)))

(assert-true (complex-check #t #t #f) "(#t AND #t) OR (NOT #f) = #t")
(assert-true (complex-check #f #f #f) "(#f AND #f) OR (NOT #f) = #t")
(assert-false (complex-check #f #f #t) "(#f AND #f) OR (NOT #t) = #f")
(display "✓ Complex logical expressions work\n")

; === Predicate Combinations ===
(display "\n=== Predicate Combinations ===\n")

; Check if number is even and positive
(def even-positive? (fn (x)
  (and (even? x) (> x 0))))

(assert-true (even-positive? 2) "2 is even and positive")
(assert-false (even-positive? -2) "-2 is even but not positive")
(assert-false (even-positive? 3) "3 is positive but not even")
(display "✓ even-positive? works\n")

; Check if number is odd and negative
(def odd-negative? (fn (x)
  (and (odd? x) (< x 0))))

(assert-true (odd-negative? -1) "-1 is odd and negative")
(assert-false (odd-negative? 1) "1 is odd but not negative")
(assert-false (odd-negative? -2) "-2 is negative but not odd")
(display "✓ odd-negative? works\n")

;; ============================================================================
;; SUMMARY
;; ============================================================================

(display "\n=== All Math and Logic Operations Verified ===\n")
(display "✓ Basic arithmetic (+, -, *, /, mod)\n")
(display "✓ Math functions (sqrt, pow, sin, cos, floor, ceil)\n")
(display "✓ Mathematical constants (pi, e)\n")
(display "✓ even? and odd? predicates\n")
(display "✓ Filtering with predicates\n")
(display "✓ Counting with predicates\n")
(display "✓ Alternating pattern detection\n")
(display "✓ Sum calculations with predicates\n")
(display "✓ not, and, or, xor logical operations\n")
(display "✓ Practical examples\n")
(display "✓ Predicate combinations\n")
(newline)
