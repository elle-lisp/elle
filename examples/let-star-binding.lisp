#!/usr/bin/elle
;; let* (let-star) Binding Example
;; Demonstrates sequential variable binding with let*
;;
;; let* differs from let in that each binding can reference previous bindings.
;; This is useful for building up values sequentially.

(begin
  (display "=== let* (Sequential Binding) ===")
  (newline)
  (newline)

  ;; Example 1: Simple sequential binding
  (display "Example 1: Simple sequential binding")
  (newline)
  (display "---")
  (newline)

  (let* ((x 10))
    (display "x = ")
    (display x)
    (newline))

  (newline)

  ;; Example 2: Multiple independent bindings
  ;; (without dependencies - both are evaluated before entering the body)
  (display "Example 2: Multiple independent bindings")
  (newline)
  (display "---")
  (newline)

  (let* ((a 5) (b 3))
    (display "a + b = ")
    (display (+ a b))
    (newline))

  (newline)

  ;; Example 3: How let* differs from let
  ;; In regular let, bindings are independent (parallel evaluation)
  ;; In let*, they're sequential
  (display "Example 3: Scope isolation")
  (newline)
  (display "---")
  (newline)

  (display "Outer scope: (define x 100)")
  (define x 100)
  (display "x = ")
  (display x)
  (newline)

  (let* ((x 5))
    (display "Inside let*: x = ")
    (display x)
    (newline))

  (display "After let*: x = ")
  (display x)
  (newline)

  (newline)

  ;; Example 4: Multiple body expressions
  (display "Example 4: Multiple expressions in body")
  (newline)
  (display "---")
  (newline)

  (let* ((count 3))
    (display "First expression: count = ")
    (display count)
    (newline)
    (display "Second expression: count * 2 = ")
    (display (* count 2))
    (newline)
    (display "Last expression (return value): count - 1 = ")
    (display (- count 1))
    (newline))

  (newline)

  ;; Example 5: Empty bindings
  (display "Example 5: Empty bindings (same as begin)")
  (newline)
  (display "---")
  (newline)

  (let* ()
    (display "let* with no bindings just sequences expressions")
    (newline))

  (newline)

  (display "=== let* Examples Complete ===")
  (newline))
