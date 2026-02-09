;;;; Exception Handling Demo - Try/Catch/Finally
;;;;
;;;; This demonstrates the user-friendly try/catch mechanism
;;;; for handling exceptions in Elle Lisp
;;;;
;;;; NOTE: There is a known Phase 9a VM issue where sequential exception-catching
;;;; in the same execution context can fail. Each example below works correctly,
;;;; but combining multiple exception-catching statements in one execution can
;;;; trigger a bug in the exception handler stack management.
;;;; See: https://github.com/disruptek/elle/issues/[tracking issue]

(display "=== Try/Catch Example 1: Safe Division ===\n")
(display "10 / 2 = ")
(display (try (/ 10 2) (catch e 0)))
(display "\n\n")

(display "=== Try/Catch Example 2: Caught Exception ===\n")
(display "10 / 0 (caught) = ")
(display (try (/ 10 0) (catch e -1)))
(display "\n\n")

(display "=== Try/Catch Example 3: No Exception ===\n")
(display "Try without exception: ")
(display (try (+ 5 3)))
(display "\n\n")

(display "=== Try/Catch Example 4: Catch Ignored on Success ===\n")
(display "(try (+ 5 3) (catch e 999)) = ")
(display (try (+ 5 3) (catch e 999)))
(display " (catch not executed)\n\n")

(display "=== Try/Catch Example 5: Finally Block ===\n")
(display "Finally always executes:\n")
(try 
  (display "  Try body\n")
  (finally (display "  Finally block\n")))

(display "\n=== Summary ===\n")
(display "try/catch/finally provides exception handling:\n")
(display "- catch: binds exception to variable for handler\n")
(display "- finally: cleanup code that always runs\n")
(display "- exception ID 4: arithmetic errors (division by zero)\n")
