; Ultra-Contrived JIT Benchmark - Tail Recursive If Expressions
; 
; This ONLY uses operations that are currently JIT-compilable:
; - Literals
; - If expressions
; - Tail recursion (using trampolining with counters)

(var countdown-ifs
  (fn (n result)
    "Tail-recursive countdown using only if expressions and literals"
    (if (= n 0)
      result
      (if (= (mod n 1000) 0)
        (begin
          (display ".")
          (countdown-ifs (- n 1) (+ result 1)))
        (countdown-ifs (- n 1) (+ result 1))))))

(display "=== Contrived JIT Benchmark ===\n\n")
(display "Test: 100,000 tail-recursive if expressions\n")
(display "This test ONLY uses:\n")
(display "  - If expressions (JIT compilable)\n")
(display "  - Literals (JIT compilable)\n")
(display "  - Comparisons and tail recursion\n\n")
(display "Running: ")

(let ((result (countdown-ifs 100000 0)))
  (display (-> "\nDone! Counted: " (append (number->string result)) (append "\n"))))

(display "\n=== Benchmark Complete ===\n")
