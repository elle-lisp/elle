;; Exception Handling Example
;; Demonstrates creating, throwing, and handling exceptions in Elle Lisp

;; Example 1: Create and inspect exceptions
(define exc1 (exception "Division by zero"))

;; Example 2: Exception with attached data
(define exc2 (exception "Invalid input" 42))

;; Example 3: Exception with complex data structure
(define error-details (list "field" "username" "reason" "taken"))
(define exc3 (exception "Validation error" error-details))

;; Example 4: Try block with successful execution
(try (+ 10 20) (catch e "error"))

;; Example 5: Try block returning the value
(define safe-value 
  (try 
    (* 5 7)
    (catch e 0)))

;; Example 6: Creating exception for later use
(define error-object (exception "Connection timeout" (list "host" "127" "port" 8080)))

;; Example 7: Multiple different exceptions
(define auth-error (exception "Authentication failed" "Invalid"))
(define network-error (exception "Network unreachable" (list "code" 503)))
(define parse-error (exception "JSON parse error" 15))

;; Example 8: Try blocks in computations
(define result1 (try (- 10 3) (catch e -1)))
(define result2 (try (/ 20 4) (catch e 0)))
(define result3 (try 100 (catch e 50)))

;; Example 9: Extract exception message
(exception-message exc1)

;; Example 10: Extract exception data
(exception-data exc2)

;; Example 11: Extract data from complex exception
(exception-data exc3)

;; Example 12: Extract data from network error
(exception-data network-error)

;; Example 13: Exception message from parse error
(exception-message parse-error)

;; Example 14: Result of arithmetic in try block
result1

;; Example 15: Another computed result
result2

;; Example 16: Third result
result3
