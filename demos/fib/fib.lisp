; Fibonacci benchmark — naive recursive, fib(30) = 832040
; Tests raw function call overhead: ~2.7M calls

(def (fib n)
  (if (< n 2) n
    (+ (fib (- n 1)) (fib (- n 2)))))

(let ((t0 (clock/monotonic)))
  (let ((result (fib 30)))
    (let ((elapsed (- (clock/monotonic) t0)))
      (display "fib(30) = ")
      (display result)
      (display "\n")
      (display "elapsed: ")
      (display (* elapsed 1000))
      (display " ms\n"))))
