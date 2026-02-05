;; Exception Filtering Patterns Example
;; Demonstrates how to create exceptions with filterable properties
;; and pattern-based error handling strategies

(display "=== Exception Filtering Patterns ===")
(newline)
(newline)

;; Example 1: Filter by error message
(display "Example 1: Filtering by error message")
(newline)

(define network-errors 
  (list 
    (exception "timeout")
    (exception "connection-refused")
    (exception "network-unreachable")))

(display "Network errors: ")
(display network-errors)
(newline)
(newline)

;; Example 2: Filter by HTTP status code
(display "Example 2: Filtering by HTTP status code")
(newline)

(define http-4xx (list
  (exception "http" 400)
  (exception "http" 401)
  (exception "http" 403)
  (exception "http" 404)))

(display "HTTP 4xx errors: ")
(display http-4xx)
(newline)

(display "Extracting codes: ")
(display (list
  (exception-data (exception "http" 400))
  (exception-data (exception "http" 404))))
(newline)
(newline)

;; Example 3: Filter by exception category
(display "Example 3: Filtering by category")
(newline)

(define auth-errors (list
  (exception "auth" "invalid-token")
  (exception "auth" "expired-session")
  (exception "auth" "missing-credentials")))

(define db-errors (list
  (exception "database" "connection-lost")
  (exception "database" "constraint-violation")
  (exception "database" "deadlock")))

(display "Auth errors: ")
(display auth-errors)
(newline)
(display "DB errors: ")
(display db-errors)
(newline)
(newline)

;; Example 4: Structured error data for filtering
(display "Example 4: Structured error data")
(newline)

(define api-error (exception "api" 
  (list 
    "endpoint" "/users"
    "method" "POST"
    "status" 422)))

(display "API error: ")
(display api-error)
(newline)
(display "Error details: ")
(display (exception-data api-error))
(newline)
(newline)

;; Example 5: Filter errors by severity
(display "Example 5: Filtering by severity")
(newline)

(define critical-errors (list
  (exception "fatal" (list "severity" "critical" "action" "shutdown"))
  (exception "fatal" (list "severity" "critical" "action" "alert"))))

(define recoverable-errors (list
  (exception "warning" (list "severity" "low" "action" "log"))
  (exception "warning" (list "severity" "low" "action" "retry"))))

(display "Critical: ")
(display critical-errors)
(newline)
(display "Recoverable: ")
(display recoverable-errors)
(newline)
(newline)

;; Example 6: Retry logic pattern
(display "Example 6: Error filtering for retry logic")
(newline)

(define transient-errors (list
  (exception "transient" (list "code" 503 "retry" #t))
  (exception "transient" (list "code" 429 "retry" #t))))

(define permanent-errors (list
  (exception "permanent" (list "code" 400 "retry" #f))
  (exception "permanent" (list "code" 401 "retry" #f))))

(display "Transient (retriable): ")
(display transient-errors)
(newline)
(display "Permanent (non-retriable): ")
(display permanent-errors)
(newline)
(newline)

;; Example 7: Validation error filtering
(display "Example 7: Validation error patterns")
(newline)

(define validation-errors (list
  (exception "validation" (list "field" "email" "error" "invalid"))
  (exception "validation" (list "field" "password" "error" "too-short"))
  (exception "validation" (list "field" "username" "error" "taken"))))

(display "Validation errors: ")
(display validation-errors)
(newline)
(newline)

;; Example 8: Filter exceptions from a list
(display "Example 8: Filtering exceptions from mixed list")
(newline)

(define all-errors (list
  (exception "network" "timeout")
  (exception "auth" "invalid")
  (exception "database" "connection-lost")
  (exception "network" "refused")
  (exception "auth" "expired")))

(display "All errors: ")
(display all-errors)
(newline)

(display "Network error count: 2 (timeout, refused)")
(newline)
(display "Auth error count: 2 (invalid, expired)")
(newline)
(display "Database error count: 1 (connection-lost)")
(newline)
(newline)

;; Example 9: Exception message patterns
(display "Example 9: Message pattern matching")
(newline)

(define timeout-msg (exception-message (exception "timeout")))
(define connection-msg (exception-message (exception "connection-refused")))
(define auth-msg (exception-message (exception "auth-failed")))

(display "Timeout message: ")
(display timeout-msg)
(newline)
(display "Connection message: ")
(display connection-msg)
(newline)
(display "Auth message: ")
(display auth-msg)
(newline)
(newline)

;; Example 10: Complex filtering strategy
(display "Example 10: Complex error handling strategy")
(newline)

;; Create exceptions that represent different error scenarios
(define http-500 (exception "http" 500))
(define http-404 (exception "http" 404))
(define network-timeout (exception "network" (list "type" "timeout" "ms" 5000)))

(display "Server error (500): ")
(display http-500)
(newline)
(display "Not found (404): ")
(display http-404)
(newline)
(display "Network timeout: ")
(display network-timeout)
(newline)
(newline)

;; Extracting codes for comparison
(display "Comparing error codes:")
(newline)
(display "500 error code: ")
(display (exception-data http-500))
(newline)
(display "404 error code: ")
(display (exception-data http-404))
(newline)
(newline)

(display "=== Exception Filtering Patterns Complete ===")
(newline)
