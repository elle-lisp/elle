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

;; ============================================================================
;; Phase 7: Exception Inheritance Matching
;; ============================================================================
;;
;; The condition system now supports exception inheritance matching.
;; When a handler specifies an exception type, it catches that type AND all subtypes.
;;
;; Exception Hierarchy:
;; - ID 1: condition (base type)
;;   └─ ID 2: error
;;      ├─ ID 3: type-error
;;      ├─ ID 4: division-by-zero
;;      ├─ ID 5: undefined-variable
;;      └─ ID 6: arity-error
;;   └─ ID 7: warning
;;      └─ ID 8: style-warning
;;
;; Inheritance Matching Examples:
;;
;; Handler for ID 2 (error) catches:
;;   - type-error (ID 3)
;;   - division-by-zero (ID 4)
;;   - undefined-variable (ID 5)
;;   - arity-error (ID 6)
;;
;; Handler for ID 1 (condition) catches EVERYTHING:
;;   - All errors (2, 3, 4, 5, 6)
;;   - All warnings (7, 8)
;;
;; Handler for ID 7 (warning) catches:
;;   - style-warning (ID 8)
;;
;; This allows writing specific handlers for specific exceptions,
;; with fallback handlers for broader categories:
;;
;; (handler-case
;;   (risky-operation)
;;   (4 (div-error)          ;; Handle division-by-zero specifically
;;     (handle-div-by-zero))
;;   (2 (other-error)        ;; Fallback for any other error
;;     (handle-generic-error))
;;   (1 (condition)          ;; Ultimate fallback for anything
;;     (handle-any-condition)))

;; ============================================================================
;; Practical Inheritance Matching Examples
;; ============================================================================

;; Safe operation that handles errors gracefully using hierarchy
(define safe-error-handler
  (lambda (x y)
    "Demonstrates error handling at different levels"
    (if (= y 0)
      (begin
        ;; Division by zero would signal exception ID 4
        ;; But we might have handlers for:
        ;; - ID 4 specifically (division-by-zero)
        ;; - ID 2 (any error)
        ;; - ID 1 (any condition)
        0)
      (/ x y))))

;; Test the safe operation
(safe-error-handler 100 5)   ;; Returns 20
(safe-error-handler 50 0)    ;; Returns 0 (protected)

;; Safe arithmetic chain with error hierarchy
(define safe-complex-calc
  (lambda (a b c)
    "Complex calculation that could fail at multiple levels"
    (if (= c 0)
      0
      (* (+ a b) (/ 100 c)))))

(safe-complex-calc 10 20 4)   ;; (10+20) * (100/4) = 750
(safe-complex-calc 5 5 0)     ;; Returns 0 (protected)

;; ============================================================================
;; Handler Dispatch with Inheritance
;; ============================================================================
;;
;; With inheritance matching, the dispatcher now works as:
;;
;; 1. Exception occurs with ID X
;; 2. For each handler checking ID Y:
;;    - Is X == Y? (exact match)
;;    - Is X a subclass of Y? (inheritance match)
;;    - If either is true, this handler matches
;; 3. First matching handler executes
;; 4. If no matches, exception propagates to outer handler

;; Example of multi-level handlers:
;; (handler-case
;;   (some-operation)
;;   ;; Specific handlers first
;;   (4 (div-e) (handle-specific))
;;   (5 (undef-e) (handle-specific))
;;   ;; General error handler
;;   (2 (err-e) (handle-any-error))
;;   ;; Ultimate fallback
;;   (1 (cond-e) (handle-any-condition)))

;; ============================================================================
;; Benefits of Exception Inheritance
;; ============================================================================
;;
;; 1. Specificity: Can handle specific exceptions with custom logic
;; 2. Generality: Can have fallback handlers for categories
;; 3. Composability: Outer handlers can catch what inner don't
;; 4. Clarity: Code clearly shows which exceptions are expected
;; 5. Maintenance: Adding new exception types doesn't break old handlers
;;
;; Example:
;; - Handler for (4 div-error) is specific: only division-by-zero
;; - Handler for (2 any-error) is broad: any error type
;; - Exceptions flow to most specific matching handler

;; Final test operations
(define result-inheritance-1 (safe-error-handler 30 3))
(define result-inheritance-2 (safe-complex-calc 15 25 5))

result-inheritance-1  ;; Should return 10
result-inheritance-2  ;; Should return 400 ((15+25) * (100/5))

;; ============================================================================
;; Phase 8: Exception Introspection and Field Access
;; ============================================================================
;;
;; Phase 8 adds primitives for inspecting exception details within handlers:
;;
;; - exception-id: Get the numeric exception ID from a Condition
;; - condition-field: Access specific field values by field ID
;; - condition-matches-type: Check if exception matches a type (with inheritance)
;; - condition-backtrace: Get backtrace information if available

;; Example: Safe operation that provides detailed error information
(define safe-divide-with-details
  (lambda (dividend divisor)
    "Safely divide, providing detailed error info on failure"
    (if (= divisor 0)
      ;; In a full handler-case, we could:
      ;; (handler-case
      ;;   (/ dividend divisor)
      ;;   (4 (exc)
      ;;     (begin
      ;;       (format "Division by zero error~n")
      ;;       (format "  Dividend: ~a~n" (condition-field exc 0))
      ;;       (format "  Divisor: ~a~n" (condition-field exc 1))
      ;;       0)))
      0
      (/ dividend divisor))))

;; Test the safe operation
(safe-divide-with-details 100 10)   ;; Returns 10
(safe-divide-with-details 50 0)     ;; Returns 0 (protected)

;; Exception introspection primitives (for use in handlers):
;;
;; (exception-id condition)
;;   Returns the numeric ID of the exception
;;   Example: (exception-id caught-error) → 4 (division-by-zero)
;;
;; (condition-field condition field-id)
;;   Returns a specific field value from the condition
;;   Example: (condition-field caught-error 0) → dividend
;;
;; (condition-matches-type condition exception-type-id)
;;   Checks if condition matches type (including inheritance)
;;   Example: (condition-matches-type caught-error 2) → #t (matches 'error')
;;
;; (condition-backtrace condition)
;;   Returns backtrace information if available
;;   Example: (condition-backtrace caught-error) → backtrace string or nil

;; ============================================================================
;; Benefits of Exception Introspection
;; ============================================================================
;;
;; 1. Detailed Error Information: Access exception fields in handlers
;; 2. Type Checking: Verify exception type match with inheritance
;; 3. Debugging: Get backtrace for debugging support
;; 4. Better Error Messages: Create custom error messages using field values
;; 5. Conditional Recovery: Different recovery based on exception details
;;
;; Example multi-level handler with introspection:
;; (handler-case
;;   (risky-operation)
;;   (4 (div-e)
;;     (format "Caught division by zero: ~a / ~a~n"
;;       (condition-field div-e 0)
;;       (condition-field div-e 1))
;;     0)
;;   (2 (err-e)
;;     (format "Caught error ~a~n" (exception-id err-e))
;;     (if (condition-backtrace err-e)
;;       (format "Backtrace: ~a~n" (condition-backtrace err-e)))
;;     0))

;; More complex safe arithmetic with better error handling
(define safe-complex-operation
  (lambda (a b c)
    "Demonstrate safe operation with potential multiple failures"
    (if (= b 0)
      0  ;; Protect against division by zero
      (if (= c 0)
        (+ a (/ 100 b))  ;; Can't divide by c, but b is safe
        (* (+ a b) (/ 100 c))))))

(safe-complex-operation 10 5 2)   ;; (10+5) * (100/2) = 750
(safe-complex-operation 10 0 2)   ;; b is 0, returns 0
(safe-complex-operation 10 5 0)   ;; c is 0, returns 10 + 20 = 30

;; ============================================================================
;; Future Enhancements (Phase 9+)
;; ============================================================================
;;
;; - Restart Mechanism: Define and invoke restarts for recovery
;; - Full try/catch integration with these introspection functions
;; - Interactive debugger with exception inspection
;; - Custom exception types with user-defined fields
;; - Exception aggregation and collection
;; - Condition filtering and routing to specialized handlers
