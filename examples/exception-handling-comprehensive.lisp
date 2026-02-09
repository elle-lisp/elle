;;;; Exception Handling in Elle Lisp - Comprehensive Reference
;;;;
;;;; This file documents the exception handling system in Elle:
;;;; - try/catch/finally: User-friendly API (Phase 10)
;;;; - handler-case: Low-level mechanism (Phase 9a)
;;;;
;;;; KNOWN LIMITATION:
;;;; There is a Phase 9a VM issue where multiple exception-catching statements
;;;; in the same execution can fail. The issue is tracked separately and being
;;;; investigated. Individual examples work correctly.

(display "=== Exception Handling Overview ===\n\n")

(display "Two levels of exception handling:\n")
(display "1. try/catch/finally (Phase 10): User-friendly\n")
(display "2. handler-case (Phase 9a): Low-level control\n\n")

(display "Exception ID 4: Arithmetic errors (division by zero, etc.)\n\n")

(display "=== Try/Catch Example ===\n")
(display "Catch division by zero: ")
(display (try (/ 10 0) (catch e 99)))
(display "\n\n")

(display "=== Handler-Case Example ===\n")
(display "Low-level exception handling: ")
(display (handler-case (+ 5 3) (4 e 0)))
(display "\n\n")

(display "=== Control Flow ===\n")
(display "When exception occurs:\n")
(display "1. Exception is created (e.g., division by zero)\n")
(display "2. Exception interrupt mechanism activates\n")
(display "3. Stack unwinds to nearest handler\n")
(display "4. Exception is bound to handler variable\n")
(display "5. Handler code executes and produces result\n")
(display "6. Exception state is cleared\n")
(display "7. Handler result is returned\n\n")

(display "=== Try/Catch Features ===\n")
(display "- catch clause: binds exception to variable\n")
(display "- finally block: always executes (cleanup code)\n")
(display "- Returns handler value when exception caught\n")
(display "- Returns body value when no exception\n\n")

(display "=== Handler-Case Features ===\n")
(display "- Explicit exception ID matching\n")
(display "- Multiple handler clauses per expression\n")
(display "- Fine-grained control over exception handling\n")
(display "- Foundation for try/catch (try/catch compiles to this)\n\n")

(display "=== Summary ===\n")
(display "Elle provides robust exception handling via two APIs.\n")
(display "Use try/catch for most code; handler-case for advanced use.\n")
(display "Both are built on the Phase 9a exception interrupt mechanism.\n")
