; JIT Benchmark - Simple If Expression Heavy Workload
;
; This focuses on operations that CAN be JIT compiled:
; - Literals
; - If expressions  
; - Begin sequences

(define if-loop
  (lambda (n)
    "Lots of if expressions with literals"
    (if (> n 0)
      (begin
        (if (= (mod n 100) 0)
          (display "."))
        (if-loop (- n 1)))
      0)))

(define nested-conditions
  (lambda (n)
    "Multiple nested if conditions"
    (if (> n 0)
      (begin
        (if (> n 1000)
          (if (> n 2000)
            (display "a")
            (display "b"))
          (if (> n 500)
            (display "c")
            (display "d")))
        (nested-conditions (- n 1)))
      0)))

(define literal-test
  (lambda (n acc)
    "Simple literal recursion"
    (if (= n 0)
      acc
      (literal-test (- n 1) (+ acc 1)))))

(display "=== JIT Benchmark Suite ===\n\n")

(display "Test 1: If-Expression Loop (5000 iterations)\n")
(display "Running: ")
(if-loop 5000)
(display "\nDone!\n\n")

(display "Test 2: Nested Conditions (3000 iterations)\n")
(display "Running: ")
(nested-conditions 3000)
(display "\nDone!\n\n")

(display "Test 3: Literal Recursion (10000 iterations)\n")
(display "Running: ")
(literal-test 10000 0)
(display "\nDone!\n\n")

(display "=== Benchmark Complete ===\n")
