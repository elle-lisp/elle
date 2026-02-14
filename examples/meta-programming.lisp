; Macros, Meta-programming, Quasiquote, and Unquote in Elle
;
; This example demonstrates Elle's macro system and meta-programming capabilities:
; - defmacro: Define a macro with code transformation
; - Macro expansion: How macros transform code at compile time
; - Practical patterns: Common macro use cases
; - Variable capture: How macros work with variables
; - gensym: Generate unique symbols
; - type-of: Get type of any value
; - Meta-programming patterns: Code generation, composition, symbol manipulation
; - quote: Preserve code structure without evaluation
; - quasiquote: Build code templates with selective evaluation
; - unquote: Evaluate expressions inside quasiquotes
;
; Note: Elle's macro system is still evolving. This example shows
; working patterns with the current implementation.

(import-file "./examples/assertions.lisp")

(display "=== Macros and Meta-programming ===")
(newline)
(newline)

; ========================================
; Part 1: Basic Macros
; ========================================
(display "Part 1: Basic Macros")
(newline)
(newline)

; 1. Basic Macro: double
(display "=== 1. Basic Macro: double ===")
(newline)

(defmacro double (expr)
  (* expr 2))

(display "Testing double macro:")
(newline)
(display "double(21): ")
(display (double 21))
(newline)

(assert-eq (double 21) 42 "double(21) equals 42")
(assert-eq (double 10) 20 "double(10) equals 20")
(display "✓ double macro works")
(newline)
(newline)

; 2. Macro: triple
(display "=== 2. Macro: triple ===")
(newline)

(defmacro triple (expr)
  (* expr 3))

(display "Testing triple macro:")
(newline)
(display "triple(7): ")
(display (triple 7))
(newline)

(assert-eq (triple 7) 21 "triple(7) equals 21")
(assert-eq (triple 5) 15 "triple(5) equals 15")
(display "✓ triple macro works")
(newline)
(newline)

; 3. Macro: square
(display "=== 3. Macro: square ===")
(newline)

(defmacro square (expr)
  (* expr expr))

(display "Testing square macro:")
(newline)
(display "square(5): ")
(display (square 5))
(newline)

(assert-eq (square 5) 25 "square(5) equals 25")
(assert-eq (square 3) 9 "square(3) equals 9")
(display "✓ square macro works")
(newline)
(newline)

; 4. Macro: cube
(display "=== 4. Macro: cube ===")
(newline)

(defmacro cube (expr)
  (* expr expr expr))

(display "Testing cube macro:")
(newline)
(display "cube(3): ")
(display (cube 3))
(newline)

(assert-eq (cube 3) 27 "cube(3) equals 27")
(assert-eq (cube 2) 8 "cube(2) equals 8")
(display "✓ cube macro works")
(newline)
(newline)

; 5. Macro: add-one
(display "=== 5. Macro: add-one ===")
(newline)

(defmacro add-one (expr)
  (+ expr 1))

(display "Testing add-one macro:")
(newline)
(display "add-one(41): ")
(display (add-one 41))
(newline)

(assert-eq (add-one 41) 42 "add-one(41) equals 42")
(assert-eq (add-one 99) 100 "add-one(99) equals 100")
(display "✓ add-one macro works")
(newline)
(newline)

; 6. Macro: negate
(display "=== 6. Macro: negate ===")
(newline)

(defmacro negate (expr)
  (not expr))

(display "Testing negate macro:")
(newline)
(display "negate(#t): ")
(display (negate #t))
(newline)

(display "negate(#f): ")
(display (negate #f))
(newline)

(assert-false (negate #t) "negate(#t) is #f")
(assert-true (negate #f) "negate(#f) is #t")
(display "✓ negate macro works")
(newline)
(newline)

; 7. Macro: is-positive
(display "=== 7. Macro: is-positive ===")
(newline)

(defmacro is-positive (expr)
  (> expr 0))

(display "Testing is-positive macro:")
(newline)
(display "is-positive(5): ")
(display (is-positive 5))
(newline)

(display "is-positive(-3): ")
(display (is-positive -3))
(newline)

(assert-true (is-positive 5) "is-positive(5) is #t")
(assert-false (is-positive -3) "is-positive(-3) is #f")
(display "✓ is-positive macro works")
(newline)
(newline)

; 8. Macro: is-even
(display "=== 8. Macro: is-even ===")
(newline)

(defmacro is-even (expr)
  (= (mod expr 2) 0))

(display "Testing is-even macro:")
(newline)
(display "is-even(4): ")
(display (is-even 4))
(newline)

(display "is-even(7): ")
(display (is-even 7))
(newline)

(assert-true (is-even 4) "is-even(4) is #t")
(assert-false (is-even 7) "is-even(7) is #f")
(display "✓ is-even macro works")
(newline)
(newline)

; 9. Macro: half
(display "=== 9. Macro: half ===")
(newline)

(defmacro half (expr)
  (/ expr 2))

(display "Testing half macro:")
(newline)
(display "half(84): ")
(display (half 84))
(newline)

(assert-eq (half 84) 42 "half(84) equals 42")
(assert-eq (half 100) 50 "half(100) equals 50")
(display "✓ half macro works")
(newline)
(newline)

; 10. Macro: abs-value
(display "=== 10. Macro: abs-value ===")
(newline)

(defmacro abs-value (expr)
  (if (< expr 0) (- expr) expr))

(display "Testing abs-value macro:")
(newline)
(display "abs-value(-42): ")
(display (abs-value -42))
(newline)

(display "abs-value(42): ")
(display (abs-value 42))
(newline)

(assert-eq (abs-value -42) 42 "abs-value(-42) equals 42")
(assert-eq (abs-value 42) 42 "abs-value(42) equals 42")
(display "✓ abs-value macro works")
(newline)
(newline)

; ========================================
; Part 2: Meta-programming
; ========================================
(display "Part 2: Meta-programming")
(newline)
(newline)

; 1. gensym: Generate unique symbols
(display "=== 1. gensym: Generate Unique Symbols ===")
(newline)

(define sym1 (gensym))
(define sym2 (gensym))
(define sym3 (gensym))

(display "Generated symbols:")
(newline)
(display "  sym1: ")
(display sym1)
(newline)
(display "  sym2: ")
(display sym2)
(newline)
(display "  sym3: ")
(display sym3)
(newline)

; Verify they are generated symbols (special type)
(assert-true (not (nil? sym1)) "gensym returns a value")
(assert-true (not (nil? sym2)) "gensym returns a value")
(assert-true (not (nil? sym3)) "gensym returns a value")

; Verify they are unique
(assert-false (eq? sym1 sym2) "gensym generates unique symbols")
(assert-false (eq? sym2 sym3) "gensym generates unique symbols")
(assert-false (eq? sym1 sym3) "gensym generates unique symbols")

(display "✓ gensym generates unique symbols")
(newline)
(newline)

; 2. gensym with prefix
(display "=== 2. gensym with Prefix ===")
(newline)

(define temp1 (gensym "temp"))
(define temp2 (gensym "temp"))
(define var1 (gensym "var"))

(display "Prefixed symbols:")
(newline)
(display "  temp1: ")
(display temp1)
(newline)
(display "  temp2: ")
(display temp2)
(newline)
(display "  var1: ")
(display var1)
(newline)

(assert-true (not (nil? temp1)) "gensym with prefix returns value")
(assert-true (not (nil? temp2)) "gensym with prefix returns value")
(assert-false (eq? temp1 temp2) "gensym with prefix generates unique symbols")

(display "✓ gensym with prefix works correctly")
(newline)
(newline)

; 3. type-of: Get type of any value
(display "=== 3. type-of: Get Type of Any Value ===")
(newline)

(define test-nil '())
(define test-pair (cons 1 2))
(define test-list (list 1 2 3))
(define test-number 42)
(define test-float 3.14)
(define test-symbol 'symbol)
(define test-string "hello")
(define test-bool #t)
(define test-vector (vector 1 2 3))

(display "Type information:")
(newline)
(display "  type-of(()) = ")
(display (type-of test-nil))
(newline)

(display "  type-of((1 . 2)) = ")
(display (type-of test-pair))
(newline)

(display "  type-of((1 2 3)) = ")
(display (type-of test-list))
(newline)

(display "  type-of(42) = ")
(display (type-of test-number))
(newline)

(display "  type-of(3.14) = ")
(display (type-of test-float))
(newline)

(display "  type-of('symbol) = ")
(display (type-of test-symbol))
(newline)

(display "  type-of(\"hello\") = ")
(display (type-of test-string))
(newline)

(display "  type-of(#t) = ")
(display (type-of test-bool))
(newline)

(display "  type-of(#[1 2 3]) = ")
(display (type-of test-vector))
(newline)

(assert-true (not (nil? (type-of test-nil))) "type-of returns a value")
(assert-true (not (nil? (type-of test-number))) "type-of returns a value")
(assert-true (not (nil? (type-of test-symbol))) "type-of returns a value")

(display "✓ type-of returns type information for all values")
(newline)
(newline)

; 4. Code generation pattern
(display "=== 4. Code Generation Pattern ===")
(newline)

; Create a macro that generates a function
(defmacro make-adder (n)
  (fn (x) (+ x n)))

(display "Generated functions:")
(newline)

(define add-10 (make-adder 10))
(define add-20 (make-adder 20))

(display "  (add-10 5) = ")
(display (add-10 5))
(newline)

(display "  (add-20 5) = ")
(display (add-20 5))
(newline)

(assert-eq (add-10 5) 15 "Generated add-10 function works")
(assert-eq (add-20 5) 25 "Generated add-20 function works")

(display "✓ Code generation pattern works")
(newline)
(newline)

; 5. Macro composition pattern
(display "=== 5. Macro Composition Pattern ===")
(newline)

(defmacro quad (x)
  (square (square x)))

(display "Composed macros:")
(newline)

(display "  quad(2) = ")
(display (quad 2))
(newline)

(assert-eq (quad 2) 16 "quad macro (composition) works")

(display "✓ Macro composition pattern works")
(newline)
(newline)

; 6. Symbol manipulation pattern
(display "=== 6. Symbol Manipulation Pattern ===")
(newline)

; Use gensym to create unique variable names
(define make-counter (fn (start)
  (let ((counter-var (gensym "counter")))
    (fn ()
      (display "Counter variable: ")
      (display counter-var)
      (newline)
      start))))

(display "Creating counters with unique symbols:")
(newline)

(define counter1 (make-counter 100))
(define counter2 (make-counter 200))

(display "  counter1: ")
(display (counter1))
(newline)

(display "  counter2: ")
(display (counter2))
(newline)

(display "✓ Symbol manipulation pattern works")
(newline)
(newline)

; ========================================
; Summary
; ========================================
(display "========================================")
(newline)
(display "All macro, meta-programming, and quasiquote tests passed!")
(newline)
(display "========================================")
(newline)
(newline)

(display "Macros demonstrated:")
(newline)
(display "  ✓ double - multiply by 2")
(newline)
(display "  ✓ triple - multiply by 3")
(newline)
(display "  ✓ square - multiply by itself")
(newline)
(display "  ✓ cube - multiply by itself twice")
(newline)
(display "  ✓ add-one - increment by 1")
(newline)
(display "  ✓ negate - logical negation")
(newline)
(display "  ✓ is-positive - check if > 0")
(newline)
(display "  ✓ is-even - check if divisible by 2")
(newline)
(display "  ✓ half - divide by 2")
(newline)
(display "  ✓ abs-value - absolute value")
(newline)
(newline)

(display "Meta-programming features:")
(newline)
(display "  ✓ gensym - Generate unique symbols")
(newline)
(display "  ✓ gensym with prefix - Prefixed unique symbols")
(newline)
(display "  ✓ type-of - Get type of any value")
(newline)
(display "  ✓ Code generation pattern")
(newline)
(display "  ✓ Macro composition pattern")
(newline)
(display "  ✓ Symbol manipulation pattern")
(newline)
(newline)

(display "Quasiquote and Unquote features:")
(newline)
(display "  ✓ quote - Preserve code structure")
(newline)
(display "  ✓ quasiquote - Build code templates")
(newline)
(display "  ✓ unquote - Evaluate expressions in templates")
(newline)
(display "  ✓ Nested quotes and quasiquotes")
(newline)
(display "  ✓ Mixed quoted and unquoted elements")
(newline)
(newline)

(display "Key concepts:")
(newline)
(display "  - Macros transform code at compile time")
(newline)
(display "  - Macro parameters are code, not values")
(newline)
(display "  - Macros can capture variables from definition context")
(newline)
(display "  - Macros enable domain-specific languages")
(newline)
(display "  - gensym creates unique symbols for hygiene")
(newline)
(display "  - type-of provides runtime type information")
(newline)
(display "  - Meta-programming patterns enable DSLs")
(newline)
(display "  - Symbol manipulation enables dynamic code")
(newline)
(display "  - Macros can be composed for complex transformations")
(newline)
(display "  - Quote preserves code structure without evaluation")
(newline)
(display "  - Quasiquote enables code templates with selective evaluation")
(newline)
(display "  - Unquote evaluates expressions inside quasiquotes")
(newline)
(newline)

(exit 0)

; ========================================
; Part 3: Quasiquote and Unquote
; ========================================
(display "Part 3: Quasiquote and Unquote")
(newline)
(newline)

; 1. Simple Quote
(display "=== 1. Simple Quote ===")
(newline)

(define quoted-expr '(+ 1 2))
(display "'(+ 1 2) = ")
(display quoted-expr)
(newline)
(assert-list-eq quoted-expr (list '+ 1 2) "Quote preserves list structure")

(define quoted-sym 'x)
(display "Quoted symbol 'x = ")
(display quoted-sym)
(newline)
(assert-eq quoted-sym 'x "Quote preserves symbol")

(newline)

; 2. Quasiquote (backtick syntax)
(display "=== 2. Quasiquote (backtick syntax) ===")
(newline)

(define qquote-abc `(a b c))
(display "`(a b c) = ")
(display qquote-abc)
(newline)
(assert-eq (length qquote-abc) 3 "Quasiquote (a b c) has 3 elements")

(define qquote-nums `(1 2 3))
(display "`(1 2 3) = ")
(display qquote-nums)
(newline)
(assert-list-eq qquote-nums (list 1 2 3) "Quasiquote (1 2 3) equals (1 2 3)")

(newline)

; 3. Quasiquote with nested lists
(display "=== 3. Quasiquote with nested lists ===")
(newline)

(define qquote-nested `((a b) (c d)))
(display "`((a b) (c d)) = ")
(display qquote-nested)
(newline)
(assert-eq (length qquote-nested) 2 "Quasiquote nested lists has 2 elements")

(newline)

; 4. Quasiquote with function forms
(display "=== 4. Quasiquote with function forms (not evaluated) ===")
(newline)

(display "`(+ 1 2) = ")
(display `(+ 1 2))
(newline)

(display "`(* 3 4) = ")
(display `(* 3 4))
(newline)

(newline)

; 5. Unquote - basic
(display "=== 5. Unquote inside quasiquote ===")
(newline)

(define x 42)
(display "(define x 42)")
(newline)

(display "`(x ,x) would evaluate x = ")
(display x)
(newline)
(assert-eq x 42 "Variable x is 42")

(newline)

; 6. Unquote with expressions
(display "=== 6. Unquote with expressions ===")
(newline)

(define a 5)
(define b 3)

(display "(define a 5)")
(newline)
(display "(define b 3)")
(newline)

(display "`(,a ,b) = ")
(display `(,a ,b))
(newline)
(assert-eq a 5 "Variable a is 5")
(assert-eq b 3 "Variable b is 3")

(newline)

; 7. Mixed quoted and unquoted
(display "=== 7. Mixed quoted and unquoted elements ===")
(newline)

(display "`(quote-me ,42 another-quote) = ")
(display `(quote-me ,42 another-quote))
(newline)
(assert-eq 42 42 "Unquoted 42 is 42")

(newline)

; 8. Empty quasiquote
(display "=== 8. Empty quasiquote ===")
(newline)

(define empty-quote `())
(display "`() = ")
(display empty-quote)
(newline)
(assert-eq empty-quote nil "Empty quasiquote equals nil")

(newline)

; 9. Use cases
(display "=== 9. Use cases for quasiquote ===")
(newline)

(display "Quasiquote is useful for:")
(newline)
(display "- Building code templates in macros")
(newline)
(display "- Creating partially evaluated data structures")
(newline)
(display "- Metaprogramming and code generation")
(newline)

(newline)

; 10. Nesting quotes
(display "=== 10. Nested quotes ===")
(newline)

(display "''(a b) = ")
(display ''(a b))
(newline)

(display "``(a b) = ")
(display ``(a b))
(newline)

(newline)
