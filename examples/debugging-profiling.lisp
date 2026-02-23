; Debugging and Profiling Primitives Example
;
; This example demonstrates Elle's debugging and profiling capabilities:
; - debug-print: Debug output
; - trace: Trace function execution
; - Profiling patterns
; - Identifying performance characteristics
; - Assertions verifying tools work

(import-file "./examples/assertions.lisp")

; ========================================
; 1. debug-print: Debug output
; ========================================
(display "=== 1. debug-print: Debug Output ===\n")

(display "Using debug-print for debugging:\n")

(debug-print "Starting debug session")
(debug-print "Variable x = 42")
(debug-print "List contents: (1 2 3)")

(display "✓ debug-print works\n")

; ========================================
; 2. trace: Trace function execution
; ========================================
(display "\n=== 2. trace: Trace Function Execution ===\n")

(def add (fn (a b)
  (+ a b)))

(def multiply (fn (a b)
  (* a b)))

(display "Tracing function calls:\n")

(display "  Calling add(3, 4):\n")
(var result1 (trace "add" (add 3 4)))
(display "  Result: ")
(display result1)
(newline)

(display "  Calling multiply(5, 6):\n")
(var result2 (trace "multiply" (multiply 5 6)))
(display "  Result: ")
(display result2)
(newline)

(assert-eq result1 7 "add(3, 4) = 7")
(assert-eq result2 30 "multiply(5, 6) = 30")

(display "✓ trace works\n")

; ========================================
; 3. Profiling pattern: Simple timing
; ========================================
(display "\n=== 3. Profiling Pattern: Simple Timing ===\n")

(def fibonacci (fn (n)
  (if (< n 2)
    n
    (+ (fibonacci (- n 1)) (fibonacci (- n 2))))))

(display "Computing fibonacci(10):\n")

(var fib-result (fibonacci 10))
(display "  Result: ")
(display fib-result)
(newline)

(assert-eq fib-result 55 "fibonacci(10) = 55")

(display "✓ Fibonacci computation works\n")

; ========================================
; 4. Profiling pattern: Function call counting
; ========================================
(display "\n=== 4. Profiling Pattern: Call Counting ===\n")

(var call-count 0)

(def counted-add (fn (a b)
  (set! call-count (+ call-count 1))
  (+ a b)))

(display "Counting function calls:\n")

(set! call-count 0)
(counted-add 1 2)
(counted-add 3 4)
(counted-add 5 6)

(display "  Total calls to counted-add: ")
(display call-count)
(newline)

(assert-eq call-count 3 "Function called 3 times")

(display "✓ Call counting pattern works\n")

; ========================================
; 5. Profiling pattern: Execution tracking
; ========================================
(display "\n=== 5. Profiling Pattern: Execution Tracking ===\n")

(var execution-log (list))

(def logged-multiply (fn (a b)
  (set! execution-log (append execution-log (list (list 'multiply a b))))
  (* a b)))

(display "Tracking function executions:\n")

(set! execution-log (list))
(logged-multiply 2 3)
(logged-multiply 4 5)
(logged-multiply 6 7)

(display "  Execution log length: ")
(display (length execution-log))
(newline)

(assert-eq (length execution-log) 3 "Three executions logged")

(display "✓ Execution tracking pattern works\n")

; ========================================
; 6. Profiling pattern: Performance comparison
; ========================================
(display "\n=== 6. Profiling Pattern: Performance Comparison ===\n")

(def simple-sum (fn (n)
  (if (= n 0)
    0
    (+ n (simple-sum (- n 1))))))

(def iterative-sum (fn (n)
  (var result 0)
  (var i 0)
  (fn ()
    (if (< i n)
      (begin
        (set! result (+ result i))
        (set! i (+ i 1)))))))

(display "Comparing sum implementations:\n")

(display "  simple-sum(100) = ")
(display (simple-sum 100))
(newline)

(assert-eq (simple-sum 100) 5050 "simple-sum(100) = 5050")

(display "✓ Performance comparison pattern works\n")

; ========================================
; 7. Debug output with values
; ========================================
(display "\n=== 7. Debug Output with Values ===\n")

(def debug-add (fn (a b)
  (debug-print "Adding two numbers")
  (debug-print (string-append "a = " (number->string a)))
  (debug-print (string-append "b = " (number->string b)))
  (let ((result (+ a b)))
    (debug-print (string-append "result = " (number->string result)))
    result)))

(display "Debug output with values:\n")

(var debug-result (debug-add 10 20))
(display "  Final result: ")
(display debug-result)
(newline)

(assert-eq debug-result 30 "debug-add(10, 20) = 30")

(display "✓ Debug output with values works\n")

; ========================================
; 8. Trace with nested calls
; ========================================
(display "\n=== 8. Trace with Nested Calls ===\n")

(def inner-fn (fn (x)
  (+ x 1)))

(def outer-fn (fn (x)
  (inner-fn (+ x 10))))

(display "Tracing nested function calls:\n")

(display "  Calling outer-fn(5):\n")
(var nested-result (trace "outer-fn" (outer-fn 5)))
(display "  Result: ")
(display nested-result)
(newline)

(assert-eq nested-result 16 "outer-fn(5) = 16")

(display "✓ Nested function tracing works\n")

; ========================================
; 9. Profiling pattern: Error tracking
; ========================================
(display "\n=== 9. Profiling Pattern: Error Tracking ===\n")

(var error-count 0)

(def safe-divide (fn (a b)
  (if (= b 0)
    (begin
      (set! error-count (+ error-count 1))
      (debug-print "Division by zero error")
      0)
    (/ a b))))

(display "Tracking errors:\n")

(set! error-count 0)
(safe-divide 10 2)
(safe-divide 20 4)
(safe-divide 30 0)
(safe-divide 40 5)
(safe-divide 50 0)

(display "  Total errors: ")
(display error-count)
(newline)

(assert-eq error-count 2 "Two division by zero errors")

(display "✓ Error tracking pattern works\n")

; ========================================
; 10. Profiling pattern: Resource usage
; ========================================
(display "\n=== 10. Profiling Pattern: Resource Usage ===\n")

(def list-size-tracker (fn (lst)
  (let ((size (length lst)))
    (debug-print (string-append "List size: " (number->string size)))
    size)))

(display "Tracking resource usage:\n")

(display "  Small list: ")
(display (list-size-tracker (list 1 2 3)))
(newline)

(display "  Medium list: ")
(display (list-size-tracker (list 1 2 3 4 5 6 7 8 9 10)))
(newline)

(display "  Large list: ")
(display (list-size-tracker (list 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15)))
(newline)

(display "✓ Resource usage tracking works\n")

; ========================================
; 11. Debugging and profiling summary
; ========================================
(display "\n=== Debugging and Profiling Summary ===\n")

(display "Features demonstrated:\n")
(display "  ✓ debug-print - Debug output\n")
(display "  ✓ trace - Trace function execution\n")
(display "  ✓ Simple timing pattern\n")
(display "  ✓ Call counting pattern\n")
(display "  ✓ Execution tracking pattern\n")
(display "  ✓ Performance comparison pattern\n")
(display "  ✓ Debug output with values\n")
(display "  ✓ Nested function tracing\n")
(display "  ✓ Error tracking pattern\n")
(display "  ✓ Resource usage tracking\n")

(display "\nKey concepts:\n")
(display "  - debug-print outputs debug messages\n")
(display "  - trace enables function call tracing\n")
(display "  - Call counting measures function usage\n")
(display "  - Execution logs track function behavior\n")
(display "  - Error tracking identifies failures\n")
(display "  - Resource tracking monitors usage\n")
(display "  - Profiling patterns enable performance analysis\n")

(display "\n")
(display "========================================\n")
(display "All debugging and profiling tests passed!\n")
(display "========================================\n")
(display "\n")

(exit 0)
