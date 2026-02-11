;; Mutual Recursion Patterns
;; Demonstrates functions that call each other in cycles
;; Elle supports mutual recursion through top-level define pre-declaration within begin blocks

(begin

(display "=== Mutual Recursion Patterns ===")
(newline)
(newline)

;; ============================================================================
;; Part 1: Even/Odd Predicates - The Classic Example
;; ============================================================================

(display "Part 1: Even/Odd Predicates")
(newline)
(display "Classic mutual recursion example")
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
(newline)

;; ============================================================================
;; Part 2: Countdown with Two Functions
;; ============================================================================

(display "Part 2: Countdown with Two Functions")
(newline)
(display "Two mutually recursive counters")
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

(count-down-a 6)
(newline)
(newline)

;; ============================================================================
;; Part 3: String Processing with Mutual Recursion
;; ============================================================================

(display "Part 3: String Processing")
(newline)
(display "Process words with mutual recursion")
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

;; ============================================================================
;; Part 4: Factorial with Helper - Mutual Style
;; ============================================================================

(display "Part 4: Factorial with Helper Function")
(newline)
(display "Mutual recursion pattern for factorial computation")
(newline)
(newline)

(define factorial
  (fn (n)
    (factorial-helper n 1)))

(define factorial-helper
  (fn (n acc)
    (if (= n 0)
      acc
      (factorial-helper (- n 1) (* acc n)))))

(display "factorial(5): ")
(display (factorial 5))
(newline)
(display "factorial(7): ")
(display (factorial 7))
(newline)
(newline)

;; ============================================================================
;; Part 5: Three-Way Mutual Recursion
;; ============================================================================

(display "Part 5: Three-Way Mutual Recursion")
(newline)
(display "Three functions calling each other")
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
(newline)

;; ============================================================================
;; Part 6: Listing Functions (Even/Odd List)
;; ============================================================================

(display "Part 6: Filtering with Mutual Recursion")
(newline)
(display "Separate even and odd numbers from a list")
(newline)
(newline)

(define separate-numbers
  (fn (nums)
    (separate-helper nums (list) (list))))

(define separate-helper
  (fn (nums evens odds)
    (if (= (length nums) 0)
      (list evens odds)
      (if (= (mod (first nums) 2) 0)
        (separate-helper (rest nums) (append evens (list (first nums))) odds)
        (separate-helper (rest nums) evens (append odds (list (first nums))))))))

(display "Input: (1 2 3 4 5 6)")
(newline)
(define separated (separate-numbers (list 1 2 3 4 5 6)))
(display "Evens: ")
(display (first separated))
(newline)
(display "Odds: ")
(display (first (rest separated)))
(newline)
(newline)

;; ============================================================================
;; Part 7: Alternating Pattern with Limited Depth
;; ============================================================================

(display "Part 7: Alternating Pattern")
(newline)
(display "Simple mutual recursion with limited depth")
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
(newline)

;; ============================================================================
;; Summary
;; ============================================================================

(display "=== Summary ===")
(newline)
(display "Mutual recursion patterns demonstrated:")
(newline)
(display "1. Even/odd predicates - classic example")
(newline)
(display "2. Alternating countdown - back and forth calls")
(newline)
(display "3. String processing - manipulating text data")
(newline)
(display "4. Factorial with helper - accumulation pattern")
(newline)
(display "5. Three-way recursion - circular function groups")
(newline)
(display "6. List filtering - conditional logic in recursion")
(newline)
(display "7. Counting patterns - mutual accumulation")
(newline)
(newline)

(display "=== Mutual Recursion Patterns Complete ===")
(newline)

) ;; End of begin block
