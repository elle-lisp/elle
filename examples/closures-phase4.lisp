;; Phase 4 Closures - Shared Mutable Captures via Cell Boxing
;; This example demonstrates Phase 4 features: lambdas can now define local
;; variables, mutate them with set!, and nested closures can capture and share
;; those mutable variables through cell boxing.

(display "=== Phase 4 Closures: Shared Mutable Captures ===")
(newline)
(newline)

;; ============================================================================
;; 1. Local Variables in Lambda Bodies
;; ============================================================================

(display "1. LOCAL VARIABLES IN LAMBDA BODIES")
(newline)
(display "--------------------------------")
(newline)

;; Before Phase 4, define inside lambda didn't work properly.
;; Now local variables are stored as cells in the closure environment.
(define compute
  (lambda ()
    (begin
      (define x 10)
      (define y 20)
      (+ x y))))

(display "Local variables: (compute) = ")
(display (compute))
(newline)
;; Expected: 30
(newline)

;; ============================================================================
;; 2. Mutation with set! Inside Lambdas
;; ============================================================================

(display "2. MUTATION WITH set! INSIDE LAMBDAS")
(newline)
(display "--------------------------------")
(newline)

;; set! now works on locally-defined variables inside lambda bodies
(define mutate-test
  (lambda ()
    (begin
      (define x 0)
      (set! x 42)
      x)))

(display "set! mutation: (mutate-test) = ")
(display (mutate-test))
(newline)
;; Expected: 42
(newline)

;; ============================================================================
;; 3. Nested Closure Capture of Local Variables
;; ============================================================================

(display "3. NESTED CLOSURE CAPTURE OF LOCAL VARIABLES")
(newline)
(display "--------------------------------")
(newline)

;; A locally-defined variable can be captured by a nested lambda
(define make-adder
  (lambda (base)
    (begin
      (define offset 10)
      (lambda (x) (+ base offset x)))))

(define add-with-offset (make-adder 100))

(display "Nested capture: (add-with-offset 5) = ")
(display (add-with-offset 5))
(newline)
;; Expected: 115
(newline)

;; ============================================================================
;; 4. Shared Mutable State — Counter Pattern
;; ============================================================================

(display "4. SHARED MUTABLE STATE — COUNTER PATTERN")
(newline)
(display "--------------------------------")
(newline)

;; The classic closure counter pattern: a factory that returns
;; a closure sharing mutable state via a cell
(define make-counter
  (lambda ()
    (begin
      (define count 0)
      (lambda ()
        (begin
          (set! count (+ count 1))
          count)))))

(define counter (make-counter))

(display "Counter sequence: ")
(display (counter))  ;; 1
(display " ")
(display (counter))  ;; 2
(display " ")
(display (counter))  ;; 3
(newline)
;; Expected: 1 2 3
(newline)

;; ============================================================================
;; 5. Independent Counters (Separate State)
;; ============================================================================

(display "5. INDEPENDENT COUNTERS (SEPARATE STATE)")
(newline)
(display "--------------------------------")
(newline)

;; Each call to make-counter creates independent state
(define c1 (make-counter))
(define c2 (make-counter))

(display "c1 sequence: ")
(display (c1)) ;; 1
(display " ")
(display (c1)) ;; 2
(newline)
;; Expected: 1 2

(display "c2 sequence: ")
(display (c2)) ;; 1 (independent!)
(newline)
;; Expected: 1
(newline)

;; ============================================================================
;; 6. Accumulator Pattern
;; ============================================================================

(display "6. ACCUMULATOR PATTERN")
(newline)
(display "--------------------------------")
(newline)

;; An accumulator that adds values to a running total
(define make-accumulator
  (lambda (initial)
    (begin
      (define total initial)
      (lambda (amount)
        (begin
          (set! total (+ total amount))
          total)))))

(define acc (make-accumulator 100))

(display "Accumulator sequence: ")
(display (acc 10))   ;; 110
(display " ")
(display (acc 20))   ;; 130
(display " ")
(display (acc 5))    ;; 135
(newline)
;; Expected: 110 130 135
(newline)

;; ============================================================================
;; 7. While Loop with set! (Issue #106 Fix)
;; ============================================================================

(display "7. WHILE LOOP WITH set! (ISSUE #106 FIX)")
(newline)
(display "--------------------------------")
(newline)

;; This was the original bug from issue #106:
;; set! inside a lambda body used to fail with "Undefined global variable"
(define sum-to-n
  (lambda (n)
    (begin
      (define result 0)
      (define i 1)
      (while (<= i n)
        (begin
          (set! result (+ result i))
          (set! i (+ i 1))))
      result)))

(display "Sum 1..10: ")
(display (sum-to-n 10))
(newline)
;; Expected: 55
(newline)

;; ============================================================================
;; 8. Multiple Closures Sharing State
;; ============================================================================

(display "8. MULTIPLE CLOSURES SHARING STATE")
(newline)
(display "--------------------------------")
(newline)

;; A pair of closures that share the same mutable cell
;; (getter and setter pattern)
;; We return multiple closures from one factory using a list.
(define make-box
  (lambda (initial)
    (begin
      (define value initial)
      (list
        (lambda () value)                          ;; getter
        (lambda (new-val) (set! value new-val))))))  ;; setter

(define box (make-box 0))
(define box-get (first box))
(define box-set (first (rest box)))

(display "Box initial: ")
(display (box-get))
(newline)
;; Expected: 0

(box-set 42)

(display "Box after set: ")
(display (box-get))
(newline)
;; Expected: 42
(newline)

;; ============================================================================
;; 9. Summary
;; ============================================================================

(display "=== SUMMARY ===")
(newline)
(display "Phase 4 enables shared mutable captures via cell boxing:")
(newline)
(display "  • Local variables in lambda bodies - define inside lambdas")
(newline)
(display "  • Mutation with set! - modify locally-defined variables")
(newline)
(display "  • Nested closure capture - inner lambdas capture outer locals")
(newline)
(display "  • Shared mutable state - closures share cells, not copies")
(newline)
(display "  • Counter pattern - classic closure-based state machines")
(newline)
(display "  • Accumulator pattern - running totals with mutable state")
(newline)
(display "  • While loops with set! - loops can mutate local variables")
(newline)
(display "  • Getter/setter pairs - multiple closures sharing one cell")
(newline)
(newline)

(display "=== All Phase 4 Examples Complete ===")
(newline)
