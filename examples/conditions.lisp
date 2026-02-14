;;; Advanced Exception Handling and Condition System

(import-file "./examples/assertions.lisp")

;;; ============================================================================
;;; Exception Inheritance Matching
;;; ============================================================================
;;;
;;; The condition system supports exception inheritance matching.
;;; When a handler specifies an exception type, it catches that type AND all subtypes.
;;;
;;; Exception Hierarchy:
;;; - ID 1: condition (base type)
;;;   └─ ID 2: error
;;;      ├─ ID 3: type-error
;;;      ├─ ID 4: division-by-zero
;;;      ├─ ID 5: undefined-variable
;;;      └─ ID 6: arity-error
;;;   └─ ID 7: warning
;;;      └─ ID 8: style-warning
;;;
;;; Inheritance Matching Examples:
;;;
;;; Handler for ID 2 (error) catches:
;;;   - type-error (ID 3)
;;;   - division-by-zero (ID 4)
;;;   - undefined-variable (ID 5)
;;;   - arity-error (ID 6)
;;;
;;; Handler for ID 1 (condition) catches EVERYTHING:
;;;   - All errors (2, 3, 4, 5, 6)
;;;   - All warnings (7, 8)
;;;
;;; Handler for ID 7 (warning) catches:
;;;   - style-warning (ID 8)

;;; ============================================================================
;;; Practical Inheritance Matching Examples
;;; ============================================================================

;; Safe operation that handles errors gracefully using hierarchy
(define safe-error-handler
  (fn (x y)
    "Demonstrates error handling at different levels"
    (if (= y 0)
      0
      (/ x y))))

;; Test the safe operation
(define safe-result-1 (safe-error-handler 100 5))
(assert-equal safe-result-1 20 "safe-error-handler: 100/5 = 20")
(define safe-result-2 (safe-error-handler 50 0))
(assert-equal safe-result-2 0 "safe-error-handler: 50/0 protected returns 0")

;; Safe arithmetic chain with error hierarchy
(define safe-complex-calc
  (fn (a b c)
    "Complex calculation that could fail at multiple levels"
    (if (= c 0)
      0
      (* (+ a b) (/ 100 c)))))

(define complex-result-1 (safe-complex-calc 10 20 4))
(assert-equal complex-result-1 750 "safe-complex-calc: (10+20)*(100/4) = 750")
(define complex-result-2 (safe-complex-calc 5 5 0))
(assert-equal complex-result-2 0 "safe-complex-calc: c=0 protected returns 0")

;;; ============================================================================
;;; Exception Introspection and Field Access
;;; ============================================================================
;;;
;;; Phase 8 adds primitives for inspecting exception details within handlers:
;;;
;;; - exception-id: Get the numeric exception ID from a Condition
;;; - condition-field: Access specific field values by field ID
;;; - condition-matches-type: Check if exception matches a type (with inheritance)
;;; - condition-backtrace: Get backtrace information if available

;; Example: Safe operation that provides detailed error information
(define safe-divide-with-details
  (fn (dividend divisor)
    "Safely divide, providing detailed error info on failure"
    (if (= divisor 0)
      0
      (/ dividend divisor))))

;; Test the safe operation
(define divide-result-1 (safe-divide-with-details 100 10))
(assert-equal divide-result-1 10 "safe-divide-with-details: 100/10 = 10")
(define divide-result-2 (safe-divide-with-details 50 0))
(assert-equal divide-result-2 0 "safe-divide-with-details: 50/0 protected returns 0")

;;; ============================================================================
;;; Exception Filtering Patterns
;;; ============================================================================
;;; Demonstrates how to create exceptions with filterable properties
;;; and pattern-based error handling strategies

;; Example 1: Filter by error message
(define network-errors 
  (list 
    (exception "timeout")
    (exception "connection-refused")
    (exception "network-unreachable")))

(assert-equal (length network-errors) 3 "Should have 3 network errors")

;; Example 2: Filter by HTTP status code
(define http-4xx (list
  (exception "http" 400)
  (exception "http" 401)
  (exception "http" 403)
  (exception "http" 404)))

(assert-equal (length http-4xx) 4 "Should have 4 HTTP 4xx errors")
(assert-equal (exception-data (exception "http" 404)) 404 "404 error code should be 404")

;; Example 3: Filter by exception category
(define auth-errors (list
  (exception "auth" "invalid-token")
  (exception "auth" "expired-session")
  (exception "auth" "missing-credentials")))

(define db-errors (list
  (exception "database" "connection-lost")
  (exception "database" "constraint-violation")
  (exception "database" "deadlock")))

(assert-equal (length auth-errors) 3 "Should have 3 auth errors")
(assert-equal (length db-errors) 3 "Should have 3 database errors")

;; Example 4: Structured error data for filtering
(define api-error (exception "api" 
  (list 
    "endpoint" "/users"
    "method" "POST"
    "status" 422)))

;; Example 5: Filter errors by severity
(define critical-errors (list
  (exception "fatal" (list "severity" "critical" "action" "shutdown"))
  (exception "fatal" (list "severity" "critical" "action" "alert"))))

(define recoverable-errors (list
  (exception "warning" (list "severity" "low" "action" "log"))
  (exception "warning" (list "severity" "low" "action" "retry"))))

;; Example 6: Retry logic pattern
(define transient-errors (list
  (exception "transient" (list "code" 503 "retry" #t))
  (exception "transient" (list "code" 429 "retry" #t))))

(define permanent-errors (list
  (exception "permanent" (list "code" 400 "retry" #f))
  (exception "permanent" (list "code" 401 "retry" #f))))

;; Example 7: Validation error filtering
(define validation-errors (list
  (exception "validation" (list "field" "email" "error" "invalid"))
  (exception "validation" (list "field" "password" "error" "too-short"))
  (exception "validation" (list "field" "username" "error" "taken"))))

;; Example 8: Filter exceptions from a list
(define all-errors (list
  (exception "network" "timeout")
  (exception "auth" "invalid")
  (exception "database" "connection-lost")
  (exception "network" "refused")
  (exception "auth" "expired")))

;; Example 9: Exception message patterns
(define timeout-msg (exception-message (exception "timeout")))
(define connection-msg (exception-message (exception "connection-refused")))
(define auth-msg (exception-message (exception "auth-failed")))

;; Example 10: Complex filtering strategy
(define http-500 (exception "http" 500))
(define http-404 (exception "http" 404))
(define network-timeout (exception "network" (list "type" "timeout" "ms" 5000)))

(assert-equal (exception-data http-500) 500 "500 error code should be 500")
(assert-equal (exception-data http-404) 404 "404 error code should be 404")

;;; ============================================================================
;;; Benefits of Exception Inheritance
;;; ============================================================================
;;;
;;; 1. Specificity: Can handle specific exceptions with custom logic
;;; 2. Generality: Can have fallback handlers for categories
;;; 3. Composability: Outer handlers can catch what inner don't
;;; 4. Clarity: Code clearly shows which exceptions are expected
;;; 5. Maintenance: Adding new exception types doesn't break old handlers

;;; ============================================================================
;;; Benefits of Exception Introspection
;;; ============================================================================
;;;
;;; 1. Detailed Error Information: Access exception fields in handlers
;;; 2. Type Checking: Verify exception type match with inheritance
;;; 3. Debugging: Get backtrace for debugging support
;;; 4. Better Error Messages: Create custom error messages using field values
;;; 5. Conditional Recovery: Different recovery based on exception details

;;; ============================================================================
;;; Assertion Summary
;;; ============================================================================
(display "=== Advanced Exception Handling and Conditions Assertions Complete ===")
(newline)
