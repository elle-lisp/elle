(elle/epoch 6)

# Fibonacci benchmark — naive recursive, fib(30) = 832040
# Tests raw function call overhead: ~2.7M calls

(defn fib [n]
  (if (< n 2) n
    (+ (fib (- n 1)) (fib (- n 2)))))

(let* ([t0 (clock/monotonic)]
       [result (fib 30)]
       [elapsed (- (clock/monotonic) t0)])
  (println "fib(30) = " result)
  (println "elapsed: " (* elapsed 1000) " ms"))
