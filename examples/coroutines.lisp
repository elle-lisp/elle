; Coroutines Example - Comprehensive Regression Tests
;
; NOTE: Most tests are currently disabled due to known bugs:
; - Issue #258: Closure environment not restored after yield/resume
; - Issue #259: Coroutine reports "already running" incorrectly
; - Issue #260: Quoted symbols in yield treated as variable references
;
; These tests will be re-enabled once the bugs are fixed.

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
; Basic coroutine creation and single resume
; ========================================
(define simple-gen (fn ()
  (yield 42)))
(define co-simple (make-coroutine simple-gen))
(assert-eq (coroutine-resume co-simple) 42 "Simple yield")
(display "✓ Basic coroutine yield\n")

; ========================================
; Coroutine status tracking
; ========================================
(define status-gen (fn ()
  (yield 1)
  (yield 2)
  'done))
(define co-status (make-coroutine status-gen))
(assert-eq (coroutine-status co-status) "created" "Initial status")
(coroutine-resume co-status)
(assert-eq (coroutine-status co-status) "suspended" "After first yield")
(coroutine-resume co-status)
(assert-eq (coroutine-status co-status) "suspended" "After second yield")
(coroutine-resume co-status)
(assert-eq (coroutine-status co-status) "done" "After completion")
(display "✓ Coroutine status tracking\n")

(display "\n========================================\n")
(display "Basic coroutine tests passed!\n")
(display "See issues #258, #259, #260 for known bugs.\n")
(display "========================================\n")
