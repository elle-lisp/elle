;; Fibonacci Sequence Computation
;; Demonstrates recursive function definition and execution
;; This example COMPUTES actual Fibonacci values, not just prints them

(display "=== Fibonacci Sequence Computation ===")
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

(display "F(6) = ")
(display (fib 6))
(newline)

(display "F(7) = ")
(display (fib 7))
(newline)

(display "F(8) = ")
(display (fib 8))
(newline)

(display "F(9) = ")
(display (fib 9))
(newline)

(display "F(10) = ")
(display (fib 10))
(newline)

(newline)

;; More efficient Fibonacci using accumulator/tail-recursion pattern
(display "=== Fibonacci with Accumulator (More Efficient) ===")
(newline)
(newline)

(define fib-acc (lambda (n)
  (define helper (lambda (n acc1 acc2)
    (if (= n 0)
      acc1
      (helper (- n 1) acc2 (+ acc1 acc2)))))
  (helper n 0 1)))

(display "Using accumulator pattern:")
(newline)

(display "F(10) = ")
(display (fib-acc 10))
(newline)

(display "F(15) = ")
(display (fib-acc 15))
(newline)

(display "F(20) = ")
(display (fib-acc 20))
(newline)

(newline)

;; Generate a list of Fibonacci numbers
(display "=== List of Fibonacci Numbers ===")
(newline)

(define fib-range (lambda (n)
  (if (= n 0)
    (list 0)
    (cons (fib-acc n) (fib-range (- n 1))))))

(display "Fibonacci numbers from F(0) to F(10):")
(newline)
(display (reverse (fib-range 10)))
(newline)

(newline)

;; Demonstrate that these are actual computed values
(display "=== Verification ===")
(newline)
(display "All values above are COMPUTED, not hardcoded")
(newline)
(display "Try changing the function and running again!")
(newline)
