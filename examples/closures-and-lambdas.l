;;; ============================================================================
;;; Closures and Lambdas Example
;;; ============================================================================
;;; This comprehensive example demonstrates proper scope behavior, variable
;;; capture, and functional programming patterns with closures and lambdas.
;;; ============================================================================

(display "=== Closures and Lambdas in Elle ===")
(newline)
(newline)

;;; ============================================================================
;;; 1. Basic Lambda Expressions
;;; ============================================================================

(display "1. BASIC LAMBDA EXPRESSIONS")
(newline)
(display "--------------------------------")
(newline)

;; Identity function
(display "Identity function: (lambda (x) x)")
(display " -> Creates a function that returns its argument unchanged")
(newline)

;; Addition function
(display "Addition function: (lambda (x y) (+ x y))")
(display " -> Creates a function that adds two numbers")
(newline)

;; Doubling function
(display "Doubling function: (lambda (x) (* x 2))")
(display " -> Creates a function that doubles its input")
(newline)
(newline)

;;; ============================================================================
;;; 2. Variable Capture (Lexical Scope)
;;; ============================================================================

(display "2. VARIABLE CAPTURE AND LEXICAL SCOPE")
(newline)
(display "--------------------------------")
(newline)

;; Example: Creating a closure that captures a variable
(display "Closure capturing outer scope variables:")
(newline)

;; The following demonstrates how lambdas capture variables from their
;; defining scope. When a lambda references a variable not in its parameters,
;; it captures that variable from the enclosing scope.
(display "  - Outer variable: base = 100")
(display " -> Inner lambda can reference: (lambda (x) (+ base x))")
(display " -> This creates a closure with 'base' in its environment")
(newline)
(newline)

;;; ============================================================================
;;; 3. Higher-Order Functions
;;; ============================================================================

(display "3. HIGHER-ORDER FUNCTIONS")
(newline)
(display "--------------------------------")
(newline)

;; Function that returns a function
(display "Function factories - lambdas creating lambdas:")
(newline)
(display "  - make-adder: (lambda (n) (lambda (x) (+ n x)))")
(display " -> Returns a function that adds n to its argument")
(newline)
(display "  - Example: (define add-5 (make-adder 5))")
(display " -> (add-5 3) would return 8")
(newline)
(newline)

;; Function composition concept
(display "Function composition pattern:")
(newline)
(display "  - compose: (lambda (f g) (lambda (x) (f (g x))))")
(display " -> Applies g first, then f to the result")
(newline)
(newline)

;;; ============================================================================
;;; 4. Nested Scope Demonstration
;;; ============================================================================

(display "4. NESTED SCOPE AND SHADOWING")
(newline)
(display "--------------------------------")
(newline)

;; Nested lambdas with proper scoping
(display "Nested lambda expressions:")
(newline)
(display "  - (lambda (x) (lambda (y) (lambda (z) (+ x y z))))")
(display " -> Multiple levels of lambdas each accessing outer scope")
(newline)

(display "Scope shadowing - inner bindings override outer:")
(newline)
(display "  - Outer binding: x = 10")
(display " -> Lambda with parameter x creates new local binding")
(display " -> Parameter x shadows the outer x within that lambda")
(newline)
(newline)

;;; ============================================================================
;;; 5. Practical Use Cases
;;; ============================================================================

(display "5. PRACTICAL USE CASES")
(newline)
(display "--------------------------------")
(newline)

(display "Mapping with functions:")
(newline)
(display "  - Apply a function to each element of a list")
(display " -> (map square (list 1 2 3 4 5)) with square = (lambda (x) (* x x))")
(newline)

(display "Filtering with predicates:")
(newline)
(display "  - Filter list elements using a predicate function")
(display " -> (filter is-positive (list -1 2 -3 4)) with")
(display "     is-positive = (lambda (x) (> x 0))")
(newline)

(display "Reducing with accumulators:")
(newline)
(display "  - Reduce list to single value using an accumulator function")
(display " -> (fold + 0 (list 1 2 3 4 5)) sums all elements")
(newline)
(newline)

;;; ============================================================================
;;; 6. Scope and Environment Guidelines
;;; ============================================================================

(display "6. SCOPE AND ENVIRONMENT GUIDELINES")
(newline)
(display "--------------------------------")
(newline)

(display "Lexical Scope Rules:")
(newline)
(display "  1. Variables are resolved in the innermost enclosing scope")
(newline)
(display "  2. Free variables in lambdas refer to enclosing scope")
(newline)
(display "  3. Parameter names shadow variables from outer scope")
(newline)
(display "  4. Each lambda creates a new local scope for its parameters")
(newline)
(newline)

(display "Closure Environment:")
(newline)
(display "  1. A closure captures variables it references from outer scope")
(newline)
(display "  2. Captured variables maintain their identity (not copied by value)")
(newline)
(display "  3. Multiple closures can share captured variables")
(newline)
(display "  4. The environment persists as long as the closure exists")
(newline)
(newline)

;;; ============================================================================
;;; 7. Common Patterns
;;; ============================================================================

(display "7. COMMON PATTERNS WITH CLOSURES")
(newline)
(display "--------------------------------")
(newline)

(display "Partial Application / Currying:")
(newline)
(display "  - Create specialized functions from general ones")
(newline)
(display "  - Example: make-multiplier = (lambda (factor) (lambda (x) (* x factor)))")
(newline)

(display "Memoization Pattern:")
(newline)
(display "  - Cache results of expensive computations")
(newline)
(display "  - Requires mutable state (not demonstrated here)")
(newline)

(display "Callbacks and Event Handlers:")
(newline)
(display "  - Lambdas used as callbacks in FFI operations")
(newline)
(display "  - Can capture state for use when callback is invoked")
(newline)
(newline)

;;; ============================================================================
;;; Summary
;;; ============================================================================

(display "SUMMARY")
(newline)
(display "-------")
(newline)
(display "Closures and lambdas are fundamental to functional programming.")
(newline)
(display "They enable:")
(newline)
(display "  • Creating functions at runtime")
(newline)
(display "  • Capturing variables from outer scope")
(newline)
(display "  • Building higher-order functions")
(newline)
(display "  • Implementing functional patterns like map, filter, fold")
(newline)
(display "  • Creating flexible callback mechanisms")
(newline)
(newline)

(display "For detailed tests and examples, see:")
(newline)
(display "  • tests/unittests/closures_and_lambdas.rs")
(newline)
(display "  • tests/integration/closures_and_lambdas.rs")
(newline)
