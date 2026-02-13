;; Coroutines and Generators in Elle
;; This example demonstrates Elle's colorless coroutine system, which provides
;; cooperative multitasking through yield/resume semantics.
;;
;; Coroutines are functions that can suspend their execution (yield) and be
;; resumed later, maintaining their internal state between suspensions.

(display "=== Coroutines and Generators in Elle ===")
(newline)
(newline)

;; ============================================================================
;; 1. BASIC YIELD/RESUME
;; ============================================================================
;; A coroutine is created from a function using make-coroutine.
;; The function can use (yield value) to suspend and return a value.
;; Use coroutine-resume to start or continue execution.

(display "1. BASIC YIELD/RESUME")
(newline)
(display "--------------------------------")
(newline)

;; Example 1a: Creating and resuming a simple coroutine
(display "Example 1a: Simple coroutine that yields once")
(newline)

;; Define a generator function that yields a single value
(define simple-gen (fn () (yield 42)))

;; Create a coroutine from the function
(define co1 (make-coroutine simple-gen))

;; Check initial status - should be "created"
(display "  Initial status: ")
(display (coroutine-status co1))
(newline)

;; Resume the coroutine to get the yielded value
(define result1 (coroutine-resume co1))
(display "  Yielded value: ")
(display result1)
(newline)

;; After yielding, status is "suspended"
(display "  Status after yield: ")
(display (coroutine-status co1))
(newline)
(newline)

;; Example 1b: Coroutine that completes without yielding
(display "Example 1b: Coroutine that returns directly (no yield)")
(newline)

(define direct-return (fn () (+ 10 20 12)))
(define co2 (make-coroutine direct-return))

(display "  Result: ")
(display (coroutine-resume co2))
(newline)

(display "  Status: ")
(display (coroutine-status co2))
(newline)

(display "  Done? ")
(display (coroutine-done? co2))
(newline)
(newline)

;; ============================================================================
;; 2. MULTIPLE SEQUENTIAL YIELDS
;; ============================================================================
;; A coroutine can yield multiple times, resuming from where it left off.

(display "2. MULTIPLE SEQUENTIAL YIELDS")
(newline)
(display "--------------------------------")
(newline)

(display "Example 2a: Three sequential yields")
(newline)

(define multi-yield-gen (fn ()
  (yield 1)
  (yield 2)
  (yield 3)))

(define multi-co (make-coroutine multi-yield-gen))

(display "  First resume: ")
(display (coroutine-resume multi-co))
(newline)

(display "  Second resume: ")
(display (coroutine-resume multi-co))
(newline)

(display "  Third resume: ")
(display (coroutine-resume multi-co))
(newline)

(display "  Status: ")
(display (coroutine-status multi-co))
(newline)
(newline)

;; Example 2b: Yields with computation between them
(display "Example 2b: Yields with computation between them")
(newline)

(define compute-gen (fn ()
  (define x 10)
  (yield x)
  (set! x (+ x 5))
  (yield x)
  (set! x (* x 2))
  (yield x)))

(define compute-co (make-coroutine compute-gen))

(display "  x=10: ")
(display (coroutine-resume compute-co))
(newline)

(display "  x=x+5: ")
(display (coroutine-resume compute-co))
(newline)

(display "  x=x*2: ")
(display (coroutine-resume compute-co))
(newline)
(newline)

;; ============================================================================
;; 3. YIELD INSIDE IF BRANCHES
;; ============================================================================

(display "3. YIELD INSIDE IF BRANCHES")
(newline)
(display "--------------------------------")
(newline)

;; Example 3a: Yield in if-then branch
(display "Example 3a: Yield in if-then-else (true branch)")
(newline)

(define cond-true-gen (fn ()
  (if #t
    (yield "then-branch")
    (yield "else-branch"))))

(define cond-true-co (make-coroutine cond-true-gen))

(display "  Condition is true, yields: ")
(display (coroutine-resume cond-true-co))
(newline)
(newline)

;; Example 3b: Yield in if-else branch
(display "Example 3b: Yield in if-then-else (false branch)")
(newline)

(define cond-false-gen (fn ()
  (if #f
    (yield "then-branch")
    (yield "else-branch"))))

(define cond-false-co (make-coroutine cond-false-gen))

(display "  Condition is false, yields: ")
(display (coroutine-resume cond-false-co))
(newline)
(newline)

;; Example 3c: Multiple yields in different branches
(display "Example 3c: Multiple yields with conditional logic")
(newline)

(define branch-gen (fn ()
  (define flag #t)
  (if flag
    (yield "first-true")
    (yield "first-false"))
  (set! flag #f)
  (if flag
    (yield "second-true")
    (yield "second-false"))))

(define branch-co (make-coroutine branch-gen))

(display "  First (flag=#t): ")
(display (coroutine-resume branch-co))
(newline)

(display "  Second (flag=#f): ")
(display (coroutine-resume branch-co))
(newline)
(newline)

;; ============================================================================
;; 4. YIELD INSIDE BEGIN BLOCKS
;; ============================================================================

(display "4. YIELD INSIDE BEGIN BLOCKS")
(newline)
(display "--------------------------------")
(newline)

(display "Example 4a: Multiple yields in begin block")
(newline)

(define begin-gen (fn ()
  (begin
    (yield "first")
    (yield "second")
    (yield "third"))))

(define begin-co (make-coroutine begin-gen))

(display "  1: ")
(display (coroutine-resume begin-co))
(newline)

(display "  2: ")
(display (coroutine-resume begin-co))
(newline)

(display "  3: ")
(display (coroutine-resume begin-co))
(newline)
(newline)

;; ============================================================================
;; 5. YIELD INSIDE WHILE LOOPS
;; ============================================================================
;; Note: Use define for state variables, not let (see Known Limitations)

(display "5. YIELD INSIDE WHILE LOOPS")
(newline)
(display "--------------------------------")
(newline)

(display "Example 5a: Counter using while loop")
(newline)

(define while-gen (fn ()
  (define i 0)
  (while (< i 3)
    (begin
      (yield i)
      (set! i (+ i 1))))
  (yield "done")))

(define while-co (make-coroutine while-gen))

(display "  i=0: ")
(display (coroutine-resume while-co))
(newline)

(display "  i=1: ")
(display (coroutine-resume while-co))
(newline)

(display "  i=2: ")
(display (coroutine-resume while-co))
(newline)

(display "  after loop: ")
(display (coroutine-resume while-co))
(newline)
(newline)

;; Example 5b: Fibonacci-like sequence in while loop
(display "Example 5b: Sequence in while loop")
(newline)

(define seq-gen (fn ()
  (define i 0)
  (while (< i 4)
    (begin
      (yield (* i i))  ;; yield squares: 0, 1, 4, 9
      (set! i (+ i 1))))))

(define seq-co (make-coroutine seq-gen))

(display "  0^2: ")
(display (coroutine-resume seq-co))
(newline)

(display "  1^2: ")
(display (coroutine-resume seq-co))
(newline)

(display "  2^2: ")
(display (coroutine-resume seq-co))
(newline)

(display "  3^2: ")
(display (coroutine-resume seq-co))
(newline)
(newline)

;; ============================================================================
;; 6. YIELD INSIDE COND
;; ============================================================================

(display "6. YIELD INSIDE COND")
(newline)
(display "--------------------------------")
(newline)

(display "Example 6a: Yield in cond clauses")
(newline)

(define cond-gen (fn ()
  (cond
    (#f (yield 1))
    (#t (yield 2))
    (else (yield 3)))))

(define cond-co (make-coroutine cond-gen))

(display "  Second clause is true, yields: ")
(display (coroutine-resume cond-co))
(newline)
(newline)

;; Example 6b: Multiple cond expressions with yields
(display "Example 6b: Multiple cond expressions")
(newline)

(define multi-cond-gen (fn ()
  (define x 1)
  (cond
    ((= x 1) (yield "one"))
    ((= x 2) (yield "two"))
    (else (yield "other")))
  (set! x 2)
  (cond
    ((= x 1) (yield "one"))
    ((= x 2) (yield "two"))
    (else (yield "other")))))

(define multi-cond-co (make-coroutine multi-cond-gen))

(display "  x=1: ")
(display (coroutine-resume multi-cond-co))
(newline)

(display "  x=2: ")
(display (coroutine-resume multi-cond-co))
(newline)
(newline)

;; ============================================================================
;; 7. COROUTINE STATUS CHECKING
;; ============================================================================

(display "7. COROUTINE STATUS CHECKING")
(newline)
(display "--------------------------------")
(newline)

(display "Example 7a: Status transitions")
(newline)

(define status-gen (fn () (yield "hello")))
(define status-co (make-coroutine status-gen))

(display "  After creation: ")
(display (coroutine-status status-co))
(newline)

(coroutine-resume status-co)

(display "  After yield: ")
(display (coroutine-status status-co))
(newline)

(coroutine-resume status-co)

(display "  After completion: ")
(display (coroutine-status status-co))
(newline)
(newline)

;; Example 7b: Type checking with coroutine?
(display "Example 7b: Type checking with coroutine?")
(newline)

(define test-gen (fn () 42))
(define test-co (make-coroutine test-gen))

(display "  (coroutine? test-co): ")
(display (coroutine? test-co))
(newline)

(display "  (coroutine? 42): ")
(display (coroutine? 42))
(newline)

(display "  (coroutine? (fn () 1)): ")
(display (coroutine? (fn () 1)))
(newline)
(newline)

;; ============================================================================
;; 8. COROUTINE-DONE? PREDICATE
;; ============================================================================

(display "8. COROUTINE-DONE? PREDICATE")
(newline)
(display "--------------------------------")
(newline)

(display "Example 8a: Checking completion state")
(newline)

(define done-gen (fn () 42))  ; No yield, completes immediately
(define done-co (make-coroutine done-gen))

(display "  Before resume - done? ")
(display (coroutine-done? done-co))
(newline)

(coroutine-resume done-co)

(display "  After resume - done? ")
(display (coroutine-done? done-co))
(newline)
(newline)

;; Example 8b: Status after yield vs completion
(display "Example 8b: Yield vs complete comparison")
(newline)

(define yield-gen (fn () (yield 1)))
(define complete-gen (fn () 2))

(define yield-co (make-coroutine yield-gen))
(define complete-co (make-coroutine complete-gen))

(coroutine-resume yield-co)
(coroutine-resume complete-co)

(display "  After yield: done? ")
(display (coroutine-done? yield-co))
(newline)

(display "  After complete: done? ")
(display (coroutine-done? complete-co))
(newline)
(newline)

;; ============================================================================
;; 9. NESTED COROUTINES
;; ============================================================================
;; Coroutines can create and resume other coroutines, enabling composition.

(display "9. NESTED COROUTINES")
(newline)
(display "--------------------------------")
(newline)

;; Example 9a: Outer coroutine that uses an inner coroutine
(display "Example 9a: Outer yields inner's value")
(newline)

(define inner-gen (fn () (yield 10)))

(define outer-gen (fn ()
  (define inner-co (make-coroutine inner-gen))
  (yield (coroutine-resume inner-co))))

(define outer-co (make-coroutine outer-gen))

(display "  Outer yields inner's value: ")
(display (coroutine-resume outer-co))
(newline)
(newline)

;; Example 9b: Three levels of nesting
(display "Example 9b: Three levels of nested coroutines")
(newline)

(define level3 (fn () (yield 3)))
(define level2 (fn ()
  (define co3 (make-coroutine level3))
  (yield (coroutine-resume co3))))
(define level1 (fn ()
  (define co2 (make-coroutine level2))
  (yield (coroutine-resume co2))))

(define co-l1 (make-coroutine level1))

(display "  Value bubbles up through 3 levels: ")
(display (coroutine-resume co-l1))
(newline)
(newline)

;; Example 9c: Using yield-from for delegation
(display "Example 9c: yield-from delegates to sub-coroutine")
(newline)

(define sub-gen (fn () (yield 100)))
(define delegating-gen (fn ()
  (define sub-co (make-coroutine sub-gen))
  (yield-from sub-co)))

(define del-co (make-coroutine delegating-gen))

(display "  yield-from result: ")
(display (coroutine-resume del-co))
(newline)
(newline)

;; ============================================================================
;; 10. PRACTICAL PATTERNS
;; ============================================================================

(display "10. PRACTICAL PATTERNS")
(newline)
(display "--------------------------------")
(newline)

;; Example 10a: Simple range generator
(display "Example 10a: Range generator (0 to 4)")
(newline)

(define range-gen (fn ()
  (define i 0)
  (while (< i 5)
    (begin
      (yield i)
      (set! i (+ i 1))))))

(define range-co (make-coroutine range-gen))

(display "  Values: ")
(display (coroutine-resume range-co))
(display " ")
(display (coroutine-resume range-co))
(display " ")
(display (coroutine-resume range-co))
(display " ")
(display (coroutine-resume range-co))
(display " ")
(display (coroutine-resume range-co))
(newline)
(newline)

;; Example 10b: Yielding complex values (lists)
(display "Example 10b: Yielding complex values (lists)")
(newline)

(define list-gen (fn ()
  (yield (list 1 2 3 4 5))))

(define list-co (make-coroutine list-gen))

(display "  Yielded list: ")
(display (coroutine-resume list-co))
(newline)
(newline)

;; Example 10c: Multiple independent coroutines
(display "Example 10c: Multiple independent coroutines")
(newline)

(define gen-a (fn () (yield "A")))
(define gen-b (fn () (yield "B")))

(define co-a (make-coroutine gen-a))
(define co-b (make-coroutine gen-b))

(display "  Resume A: ")
(display (coroutine-resume co-a))
(newline)

(display "  Resume B: ")
(display (coroutine-resume co-b))
(newline)
(newline)

;; ============================================================================
;; SUMMARY
;; ============================================================================

(display "=== SUMMARY ===")
(newline)
(newline)
(display "Coroutine primitives in Elle:")
(newline)
(display "  make-coroutine    - Create coroutine from function")
(newline)
(display "  coroutine-resume  - Start or continue execution")
(newline)
(display "  coroutine-status  - Get status (created/running/suspended/done/error)")
(newline)
(display "  coroutine-done?   - Check if coroutine completed")
(newline)
(display "  coroutine-value   - Get last yielded value")
(newline)
(display "  coroutine?        - Check if value is a coroutine")
(newline)
(display "  yield             - Suspend and return a value")
(newline)
(display "  yield-from        - Delegate to sub-coroutine")
(newline)
(newline)

;; ============================================================================
;; KNOWN LIMITATIONS
;; ============================================================================
;; The following patterns do not currently work with coroutines.
;; See the linked issues for details and progress.

(display "=== KNOWN LIMITATIONS ===")
(newline)
(newline)

(display "#251: yield inside let/let* fails on resume")
(newline)
(display "  https://github.com/adavidoff/elle/issues/251")
(newline)
(display "  Workaround: Use (define ...) instead of (let ...)")
(newline)
(display "  Example that fails:")
(newline)
(display "    (define gen (fn () (let ((x 10)) (yield x) (yield (+ x 1)))))")
(newline)
(display "    ;; First resume works, second resume fails with 'Undefined global'")
(newline)
(newline)

(display "#252: calling a function that yields fails on resume")
(newline)
(display "  https://github.com/adavidoff/elle/issues/252")
(newline)
(display "  Workaround: Inline the yielding code in the coroutine body")
(newline)
(display "  Example that fails:")
(newline)
(display "    (define helper (fn () (yield 42)))")
(newline)
(display "    (define gen (fn () (helper) (yield \"after\")))")
(newline)
(display "    ;; Error: 'yield used outside of coroutine'")
(newline)
(newline)

(display "#253: lambdas inside coroutine body not supported")
(newline)
(display "  https://github.com/adavidoff/elle/issues/253")
(newline)
(display "  Workaround: Define helper functions outside the coroutine")
(newline)
(display "  Example that fails:")
(newline)
(display "    (define gen (fn () (define f (fn (x) (+ x 1))) (yield (f 10))))")
(newline)
(display "    ;; Error: 'Pure expression type not yet supported: Lambda'")
(newline)
(newline)

(display "#254: recursive functions with yield fail after first yield")
(newline)
(display "  https://github.com/adavidoff/elle/issues/254")
(newline)
(display "  Workaround: Use while loops instead of recursion")
(newline)
(display "  Example that fails:")
(newline)
(display "    (define gen (fn () (define loop (fn () (yield 1) (loop))) (loop)))")
(newline)
(display "    ;; First resume works, second fails with 'resume_from_context called outside coroutine'")
(newline)
(newline)

(display "=== Coroutines Example Complete ===")
(newline)
