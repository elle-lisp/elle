# Fibonacci benchmark — naive recursive, fib(30) = 832040

(defn fib [n]
  (if (< n 2) n
    (+ (fib (- n 1)) (fib (- n 2)))))

(def t0 (os/clock))
(def result (fib 30))
(def elapsed (- (os/clock) t0))

(printf "fib(30) = %d" result)
(printf "elapsed: %.2f ms" (* elapsed 1000))
