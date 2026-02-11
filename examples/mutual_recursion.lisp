;; Mutual recursion with letrec
(letrec ((is-even (fn (n)
                    (if (= n 0) #t (is-odd (- n 1)))))
         (is-odd (fn (n)
                   (if (= n 0) #f (is-even (- n 1))))))
  (begin
    (display "10 is even: ") (display (is-even 10)) (newline)
    (display "7 is odd: ") (display (is-odd 7)) (newline)))

;; Three-way mutual recursion
(letrec ((fizz (fn (n)
                 (if (= n 0) "fizz" (buzz (- n 1)))))
         (buzz (fn (n)
                 (if (= n 0) "buzz" (fizzbuzz (- n 1)))))
         (fizzbuzz (fn (n)
                     (if (= n 0) "fizzbuzz" (fizz (- n 1))))))
  (begin
    (display "fizz(0): ") (display (fizz 0)) (newline)
    (display "fizz(1): ") (display (fizz 1)) (newline)
    (display "fizz(2): ") (display (fizz 2)) (newline)
    (display "fizz(5): ") (display (fizz 5)) (newline)))

;; Tail-recursive deep computation
(define sum-to (fn (n acc)
  (if (= n 0) acc (sum-to (- n 1) (+ acc n)))))

(display "Sum 1..1000: ") (display (sum-to 1000 0)) (newline)
