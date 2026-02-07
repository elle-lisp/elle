#!/usr/bin/elle
;; Quasiquote and Unquote Example
;; Demonstrates quote, quasiquote, and unquote for metaprogramming

(begin
  (display "=== Quasiquote and Unquote Examples ===")
  (newline)
  (newline)

  ;; Example 1: Simple Quote
  (display "Example 1: Simple Quote")
  (newline)
  (display "---")
  (newline)

  (display "'(+ 1 2) = ")
  (display '(+ 1 2))
  (newline)

  (display "Quoted symbol 'x = ")
  (display 'x)
  (newline)

  (newline)

  ;; Example 2: Quasiquote (backtick syntax)
  (display "Example 2: Quasiquote (backtick syntax)")
  (newline)
  (display "---")
  (newline)

  (display "`(a b c) = ")
  (display `(a b c))
  (newline)

  (display "`(1 2 3) = ")
  (display `(1 2 3))
  (newline)

  (newline)

  ;; Example 3: Quasiquote with nested lists
  (display "Example 3: Quasiquote with nested lists")
  (newline)
  (display "---")
  (newline)

  (display "`((a b) (c d)) = ")
  (display `((a b) (c d)))
  (newline)

  (newline)

  ;; Example 4: Quasiquote with function forms
  (display "Example 4: Quasiquote with function forms (not evaluated)")
  (newline)
  (display "---")
  (newline)

  (display "`(+ 1 2) = ")
  (display `(+ 1 2))
  (newline)

  (display "`(* 3 4) = ")
  (display `(* 3 4))
  (newline)

  (newline)

  ;; Example 5: Unquote - basic
  (display "Example 5: Unquote inside quasiquote")
  (newline)
  (display "---")
  (newline)

  (define x 42)
  (display "(define x 42)")
  (newline)

  (display "`(x ,x) would evaluate x = ")
  (display x)
  (newline)

  (newline)

  ;; Example 6: Unquote with expressions
  (display "Example 6: Unquote with expressions")
  (newline)
  (display "---")
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

  (newline)

  ;; Example 7: Mixed quoted and unquoted
  (display "Example 7: Mixed quoted and unquoted elements")
  (newline)
  (display "---")
  (newline)

  (display "`(quote-me ,42 another-quote) = ")
  (display `(quote-me ,42 another-quote))
  (newline)

  (newline)

  ;; Example 8: Empty quasiquote
  (display "Example 8: Empty quasiquote")
  (newline)
  (display "---")
  (newline)

  (display "`() = ")
  (display `())
  (newline)

  (newline)

  ;; Example 9: Use cases
  (display "Example 9: Use cases for quasiquote")
  (newline)
  (display "---")
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

  ;; Example 10: Nesting quotes
  (display "Example 10: Nested quotes")
  (newline)
  (display "---")
  (newline)

  (display "''(a b) = ")
  (display ''(a b))
  (newline)

  (display "``(a b) = ")
  (display ``(a b))
  (newline)

  (newline)

  (display "=== Quasiquote and Unquote Examples Complete ===")
  (newline))
