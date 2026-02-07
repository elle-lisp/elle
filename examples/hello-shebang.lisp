#!/usr/bin/env elle
;; Hello World - Executable Script Example
;; This script demonstrates shebang support in Elle Lisp
;; Make this file executable with: chmod +x hello-shebang.lisp
;; Then run it directly: ./hello-shebang.lisp

;; Simple greeting
(display "Hello from Elle Lisp!")
(newline)

;; Demonstrate computation
(display "Result: ")
(display (+ 10 20 30))
(newline)

;; List operations
(display "List: ")
(display (list "one" "two" "three"))
(newline)

;; Function composition
(display "Factorial of 5: ")
(display (* 5 4 3 2 1))
(newline)

;; End message
(display "Script execution complete!")
(newline)
