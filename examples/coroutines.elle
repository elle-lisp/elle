; Coroutines Example - Comprehensive Regression Tests
; This file serves as both documentation and CI regression test.
; Any failure will exit with code 1, failing CI.

; === Helper for assertions ===
(define assert-eq (fn (actual expected msg)
  (let ((matches
    (if (symbol? expected)
        (eq? actual expected)
        (= actual expected))))
    (if matches
        #t
        (begin
          (display "FAIL: ")
          (display msg)
          (display "\n  Expected: ")
          (display expected)
          (display "\n  Actual: ")
          (display actual)
          (display "\n")
          (exit 1))))))

; ========================================
; Issue #251: Let bindings across yield
; ========================================
(define gen-251 (fn ()
  (let ((x 10))
    (yield x)
    (yield (+ x 1)))))
(define co-251 (make-coroutine gen-251))
(assert-eq (coroutine-resume co-251) 10 "Issue #251: first yield")
(assert-eq (coroutine-resume co-251) 11 "Issue #251: second yield")
(display "✓ Issue #251: Let bindings across yield\n")

; ========================================
; Issue #252: Nested yielding function calls
; ========================================
(define helper-252 (fn () (yield 42)))
(define gen-252 (fn () (helper-252) (yield 100)))
(define co-252 (make-coroutine gen-252))
(assert-eq (coroutine-resume co-252) 42 "Issue #252: inner yield")
(assert-eq (coroutine-resume co-252) 100 "Issue #252: outer yield")
(display "✓ Issue #252: Nested yielding function calls\n")

; ========================================
; Issue #253: Lambda inside coroutine
; ========================================
(define gen-253 (fn ()
  (let ((f (fn (x) (* x 2))))
    (yield (f 10)))))
(define co-253 (make-coroutine gen-253))
(assert-eq (coroutine-resume co-253) 20 "Issue #253: lambda result")
(display "✓ Issue #253: Lambda inside coroutine\n")

; ========================================
; Issue #254: Recursive yielding function
; ========================================
(define countdown (fn (n)
  (if (> n 0)
      (begin (yield n) (countdown (- n 1)))
      (yield 0))))
(define co-254 (make-coroutine (fn () (countdown 3))))
(assert-eq (coroutine-resume co-254) 3 "Issue #254: countdown 3")
(assert-eq (coroutine-resume co-254) 2 "Issue #254: countdown 2")
(assert-eq (coroutine-resume co-254) 1 "Issue #254: countdown 1")
(assert-eq (coroutine-resume co-254) 0 "Issue #254: countdown 0")
(display "✓ Issue #254: Recursive yielding function\n")

; ========================================
; Deeply nested let bindings with yields
; ========================================
(define gen-deep (fn ()
  (let ((a 1))
    (yield a)
    (let ((b 2))
      (yield (+ a b))))))
(define co-deep (make-coroutine gen-deep))
(assert-eq (coroutine-resume co-deep) 1 "Deep let: a=1")
(assert-eq (coroutine-resume co-deep) 3 "Deep let: a+b=3")
(display "✓ Deeply nested let bindings\n")

; ========================================
; Closure capturing across yield
; ========================================
(define make-counter (fn (start)
  (fn ()
    (yield start)
    (yield (+ start 1))
    (yield (+ start 2)))))
(define co-100 (make-coroutine (make-counter 100)))
(define co-200 (make-coroutine (make-counter 200)))
(assert-eq (coroutine-resume co-100) 100 "Counter 100: first")
(assert-eq (coroutine-resume co-200) 200 "Counter 200: first")
(assert-eq (coroutine-resume co-100) 101 "Counter 100: second")
(assert-eq (coroutine-resume co-200) 201 "Counter 200: second")
(display "✓ Closure capturing across yield\n")

; ========================================
; Multiple coroutines interleaved
; ========================================
(define gen-a (fn () (yield 1) (yield 2) (yield 3)))
(define gen-b (fn () (yield 'a) (yield 'b) (yield 'c)))
(define co-a (make-coroutine gen-a))
(define co-b (make-coroutine gen-b))
(assert-eq (coroutine-resume co-a) 1 "Interleaved: a1")
(assert-eq (coroutine-resume co-b) 'a "Interleaved: b1")
(assert-eq (coroutine-resume co-a) 2 "Interleaved: a2")
(assert-eq (coroutine-resume co-b) 'b "Interleaved: b2")
(assert-eq (coroutine-resume co-a) 3 "Interleaved: a3")
(assert-eq (coroutine-resume co-b) 'c "Interleaved: b3")
(display "✓ Multiple coroutines interleaved\n")

; ========================================
; Yield inside control flow
; ========================================
(define gen-if (fn ()
  (let ((x 5))
    (if (> x 3)
        (yield 'greater)
        (yield 'lesser))
    (yield 'done))))
(define co-if (make-coroutine gen-if))
(assert-eq (coroutine-resume co-if) 'greater "Control flow: if")
(assert-eq (coroutine-resume co-if) 'done "Control flow: after")
(display "✓ Yield inside control flow\n")

; ========================================
; Coroutine status tracking
; ========================================
(define gen-status (fn () (yield 1) (yield 2)))
(define co-status (make-coroutine gen-status))
(assert-eq (coroutine-status co-status) "created" "Status: created")
(coroutine-resume co-status)
(assert-eq (coroutine-status co-status) "suspended" "Status: suspended")
(coroutine-resume co-status)
(assert-eq (coroutine-status co-status) "suspended" "Status: still suspended")
(coroutine-resume co-status)
(assert-eq (coroutine-status co-status) "done" "Status: done")
(display "✓ Coroutine status tracking\n")

; ========================================
; All tests passed!
; ========================================
(display "\n========================================\n")
(display "All coroutine tests passed!\n")
(display "========================================\n")
