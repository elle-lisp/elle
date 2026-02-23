; Fibonacci benchmark — naive recursive, fib(30) = 832040

(define (fib n)
  (if (< n 2) n
    (+ (fib (- n 1)) (fib (- n 2)))))

(let* ((t0 (current-time))
       (result (fib 30))
       (t1 (current-time))
       (elapsed (+ (- (time-second t1) (time-second t0))
                   (/ (- (time-nanosecond t1) (time-nanosecond t0))
                      1000000000.0))))
  (display "fib(30) = ")
  (display result)
  (newline)
  (display "elapsed: ")
  (display (* elapsed 1000))
  (display " ms")
  (newline))
