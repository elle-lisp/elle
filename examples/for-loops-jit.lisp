;; For Loops with JIT Compilation Support
;;
;; This example demonstrates For loops with a focus on JIT compilation
;; capabilities in the Elle Lisp compiler.
;;
;; JIT Note: For loops over literal lists are compiled by unrolling the
;; loop at compile time, eliminating runtime iteration overhead.

(display "=== For Loops with JIT Support ===")
(newline)
(newline)

;; ============================================================================
;; Part 1: For Loop Basics (JIT Compiled)
;; ============================================================================

(display "Part 1: For Loop Basics (Literal List Unrolling)")
(newline)
(display "For loops over literal lists are unrolled at compile time")
(newline)
(newline)

(display "Example 1a: Simple iteration over literal list")
(newline)
(define result1 (list))
(for x (list 1 2 3)
  (begin (display x) (display " ")))
(newline)
(newline)

(display "Example 1b: Processing elements from literal list")
(newline)
(for item (list 10 20 30 40 50)
  (begin (display item) (display " ")))
(newline)
(newline)

;; ============================================================================
;; Part 2: For Loop with Empty List
;; ============================================================================

(display "Part 2: For Loop with Empty List")
(newline)
(display "For loop over empty list executes zero times and returns nil")
(newline)
(newline)

(display "Example 2a: Empty list iteration")
(newline)
(define empty-result (for item (list)
  (display "This will not print")))
(display "Result: ")
(display empty-result)
(newline)
(newline)

;; ============================================================================
;; Part 3: Nested For Loops with Literal Lists
;; ============================================================================

(display "Part 3: Nested For Loops (Both Unrolled)")
(newline)
(display "Nested for loops over literal lists are both unrolled")
(newline)
(newline)

(display "Example 3a: 2D grid iteration")
(newline)
(for row (list 1 2 3)
  (begin
    (for col (list 1 2)
      (begin
        (display "(")
        (display row)
        (display ",")
        (display col)
        (display ") ")))
    (newline)))
(newline)

;; ============================================================================
;; Part 4: For Loop with Expressions
;; ============================================================================

(display "Part 4: For Loops with Expressions")
(newline)
(display "Body expressions are compiled for each iteration")
(newline)
(newline)

(display "Example 4a: Arithmetic in loop body")
(newline)
(for n (list 1 2 3 4 5)
  (begin (display (* n n)) (display " ")))
(newline)
(newline)

(display "Example 4b: Conditional logic in loop body")
(newline)
(for n (list 1 2 3 4 5)
  (if (> n 2)
    (display n)
    (display "-")))
(newline)
(newline)

;; ============================================================================
;; Part 5: Multiple For Loops
;; ============================================================================

(display "Part 5: Multiple Sequential For Loops")
(newline)
(display "Each for loop is independently unrolled")
(newline)
(newline)

(display "Example 5a: First loop")
(newline)
(for x (list 10 11 12)
  (begin (display x) (display " ")))
(newline)

(display "Example 5b: Second loop")
(newline)
(for x (list 1 2 3)
  (display x))
(newline)
(newline)

;; ============================================================================
;; Part 6: For Loops vs While Loops
;; ============================================================================

(display "Part 6: For vs While Loops")
(newline)
(display "Comparing literal list iteration patterns")
(newline)
(newline)

(display "Example 6a: For loop (JIT unrolled) over literal list")
(newline)
(for val (list 10 20 30)
  (begin (display val) (display " ")))
(newline)

(display "Example 6b: While loop (iterates at runtime)")
(newline)
(define lst (list 10 20 30))
(define idx 0)
(while (< idx (length lst))
  (begin
    (display (nth idx lst))
    (display " ")
    (set! idx (+ idx 1))))
(newline)
(newline)

(display "Note: For loops over literal lists are compiled away,")
(newline)
(display "while while loops execute at runtime.")
(newline)
(newline)

;; ============================================================================
;; Part 7: Performance Implications
;; ============================================================================

(display "Part 7: Performance Characteristics")
(newline)
(newline)

(display "For loops over LITERAL lists:")
(newline)
(display "  - Unrolled at compile time")
(newline)
(display "  - No runtime iteration overhead")
(newline)
(display "  - Body executed inline for each element")
(newline)
(newline)

(display "For loops over COMPUTED lists (variable references):")
(newline)
(display "  - Not yet supported in JIT")
(newline)
(display "  - Requires variable binding support in JIT compiler")
(newline)
(display "  - Falls back to bytecode interpretation")
(newline)
(newline)

;; ============================================================================
;; Summary
;; ============================================================================

(display "=== Summary ===")
(newline)
(display "JIT-Compiled For Loops:")
(newline)
(display "1. Literal list loops are unrolled at compile time")
(newline)
(display "2. Empty lists result in zero iterations")
(newline)
(display "3. Nested loops are both unrolled independently")
(newline)
(display "4. Body expressions compiled for each unrolled iteration")
(newline)
(display "5. Computed lists require bytecode interpretation")
(newline)
(newline)

(display "=== For Loops with JIT Support Complete ===")
(newline)
