#!/usr/bin/env elle

;; Self-recursive define inside lambda (Issue #179)
(define run-factorial (lambda (n)
  (begin
    (define fact (lambda (x) (if (= x 0) 1 (* x (fact (- x 1))))))
    (fact n))))

(display "Factorial of 6: ")
(display (run-factorial 6))
(newline)

;; Mutual recursion with define inside lambda (Issue #180)
(define run-even-odd (lambda ()
  (begin
    (define is-even (lambda (n) (if (= n 0) #t (is-odd (- n 1)))))
    (define is-odd (lambda (n) (if (= n 0) #f (is-even (- n 1)))))
    (is-even 8))))

(display "Is 8 even? ")
(display (run-even-odd))
(newline)
