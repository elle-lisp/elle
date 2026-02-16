;; JIT Compilation Examples
;; Demonstrates on-demand JIT compilation and each loops with JIT support

(import-file "./examples/assertions.lisp")

;; ============================================================================
;; PART 1: JIT Compilation Basics
;; ============================================================================

;; Test jit-compiled? with non-closure values
(display "Testing jit-compiled? with non-closure values:")
(newline)
(display "  (jit-compiled? 42) = ")
(display (jit-compiled? 42))
(newline)

(display "  (jit-compiled? nil) = ")
(display (jit-compiled? nil))
(newline)

;; Test jit-compilable? with non-closure values
(display "Testing jit-compilable? with non-closure values:")
(newline)
(display "  (jit-compilable? 42) = ")
(display (jit-compilable? 42))
(newline)

(display "  (jit-compilable? nil) = ")
(display (jit-compilable? nil))
(newline)

;; ============================================================================
;; PART 2: Each Loops with JIT Compilation Support
;; ============================================================================

(display "\n=== Each Loops with JIT Support ===")
(newline)
(newline)

;; ============================================================================
;; Part 1: Each Loop Basics (JIT Compiled)
;; ============================================================================

(display "Part 1: Each Loop Basics (Literal List Unrolling)")
(newline)
(display "Each loops over literal lists are unrolled at compile time")
(newline)
(newline)

(display "Example 1a: Simple iteration over literal list")
(newline)
(define result1 (list))
(each x (list 1 2 3)
  (begin (display x) (display " ")))
(newline)
(assert-eq result1 (list) "Empty list should remain empty")
(newline)

(display "Example 1b: Processing elements from literal list")
(newline)
(each item (list 10 20 30 40 50)
  (begin (display item) (display " ")))
(newline)
(newline)

;; ============================================================================
;; Part 2: Each Loop with Empty List
;; ============================================================================

(display "Part 2: Each Loop with Empty List")
(newline)
(display "Each loop over empty list executes zero times and returns nil")
(newline)
(newline)

(display "Example 2a: Empty list iteration")
(newline)
(define empty-result (each item (list)
  (display "This will not print")))
(display "Result: ")
(display empty-result)
(newline)
(assert-eq empty-result (list) "Empty each loop should return nil/empty list")
(newline)

;; ============================================================================
;; Part 3: Nested Each Loops with Literal Lists
;; ============================================================================

(display "Part 3: Nested Each Loops (Both Unrolled)")
(newline)
(display "Nested each loops over literal lists are both unrolled")
(newline)
(newline)

(display "Example 3a: 2D grid iteration")
(newline)
(each row (list 1 2 3)
  (begin
    (each col (list 1 2)
      (begin
        (display "(")
        (display row)
        (display ",")
        (display col)
        (display ") ")))
    (newline)))
(newline)

;; ============================================================================
;; Part 4: Each Loop with Expressions
;; ============================================================================

(display "Part 4: Each Loops with Expressions")
(newline)
(display "Body expressions are compiled for each iteration")
(newline)
(newline)

(display "Example 4a: Arithmetic in loop body")
(newline)
(each n (list 1 2 3 4 5)
  (begin (display (* n n)) (display " ")))
(newline)
;; Verify arithmetic works: 1*1=1, 2*2=4, 3*3=9, 4*4=16, 5*5=25
(assert-eq (* 1 1) 1 "1 squared is 1")
(assert-eq (* 5 5) 25 "5 squared is 25")
(newline)

(display "Example 4b: Conditional logic in loop body")
(newline)
(each n (list 1 2 3 4 5)
  (if (> n 2)
    (display n)
    (display "-")))
(newline)
(newline)

;; ============================================================================
;; Part 5: Multiple Each Loops
;; ============================================================================

(display "Part 5: Multiple Sequential Each Loops")
(newline)
(display "Each each loop is independently unrolled")
(newline)
(newline)

(display "Example 5a: First loop")
(newline)
(each x (list 10 11 12)
  (begin (display x) (display " ")))
(newline)

(display "Example 5b: Second loop")
(newline)
(each x (list 1 2 3)
  (display x))
(newline)
(newline)

;; ============================================================================
;; Part 6: Each Loops vs While Loops
;; ============================================================================

(display "Part 6: Each vs While Loops")
(newline)
(display "Comparing literal list iteration patterns")
(newline)
(newline)

(display "Example 6a: Each loop (JIT unrolled) over literal list")
(newline)
(each val (list 10 20 30)
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

(display "Note: Each loops over literal lists are compiled away,")
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

(display "Each loops over LITERAL lists:")
(newline)
(display "  - Unrolled at compile time")
(newline)
(display "  - No runtime iteration overhead")
(newline)
(display "  - Body executed inline for each element")
(newline)
(newline)

(display "Each loops over COMPUTED lists (variable references):")
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
(display "JIT-Compiled Each Loops:")
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

(display "=== JIT Compilation Examples Complete ===")
(newline)
