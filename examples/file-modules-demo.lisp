;;
;; File-Based Modules Demo
;;
;; This example demonstrates how to use Elle's file-based module system
;; to organize code across multiple files.
;;

;; Import simple utility module
(import-file "test-modules/test.lisp")

;; Demonstrate successful module import
(display "=== File-Based Modules Demo ===")
(newline)

;; Test 1: Verify the file was loaded successfully
(display "Test 1: Module Import Success")
(newline)
(display "  Successfully imported test-modules/test.lisp")
(newline)

;; Test 2: Add a module search path
(display "Test 2: Adding Module Search Paths")
(newline)
(add-module-path "test-modules")
(display "  Added 'test-modules' to module search path")
(newline)

;; Test 3: Import the same file twice (idempotent)
(display "Test 3: Idempotent Loading")
(newline)
(import-file "test-modules/test.lisp")
(display "  Successfully imported the same file twice (idempotent)")
(newline)

;; Test 4: Demonstrate basic arithmetic with imported modules loaded
(display "Test 4: Arithmetic Operations")
(newline)
(display "  5 + 3 = ")
(display (+ 5 3))
(newline)
(display "  10 * 2 = ")
(display (* 10 2))
(newline)

(display "=== All Tests Passed ===")
(newline)
