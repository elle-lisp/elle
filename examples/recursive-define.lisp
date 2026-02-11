#!/usr/bin/env elle

;; Self-recursive define inside fn (Issue #179)
(define run-factorial (fn (n)
  (begin
    (define fact (fn (x) (if (= x 0) 1 (* x (fact (- x 1))))))
    (fact n))))

(display "Factorial of 6: ")
(display (run-factorial 6))
(newline)

;; Mutual recursion with define inside fn (Issue #180)
(define run-even-odd (fn ()
  (begin
    (define is-even (fn (n) (if (= n 0) #t (is-odd (- n 1)))))
    (define is-odd (fn (n) (if (= n 0) #f (is-even (- n 1)))))
    (is-even 8))))

(display "Is 8 even? ")
(display (run-even-odd))
(newline)
