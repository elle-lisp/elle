;; Elle Assertions Library
;;
;; Shared assertion helpers for all examples. Load this file with:
;;   (import-file "./examples/assertions.lisp")
;;
;; Functions:
;;   - assert-eq(actual, expected, msg)
;;     Assert that actual equals expected (using = for numbers, eq? for symbols)
;;   - assert-equal(actual, expected, msg)
;;     Alias for assert-eq
;;   - assert-true(val, msg)
;;     Assert that val is true
;;   - assert-false(val, msg)
;;     Assert that val is false
;;   - assert-list-eq(actual, expected, msg)
;;     Assert that two lists are equal (same length and elements)
;;   - assert-not-nil(val, msg)
;;     Assert that val is not nil
;;   - assert-string-eq(actual, expected, msg)
;;     Assert that two strings are equal
;;
;; All assertions crash with exit code 1 on failure, making examples
;; act as contracts for the implementation.

(def assert-eq (fn (actual expected msg)
  "Assert that actual equals expected (using = for numbers, eq? for symbols)"
  (let ((matches
    (if (symbol? expected)
        (eq? actual expected)
        (= actual expected))))
    (if matches
        true
        (begin
          (display "FAIL: ")
          (display msg)
          (display "\n  Expected: ")
          (display expected)
          (display "\n  Actual: ")
          (display actual)
          (display "\n")
          (exit 1))))))

(def assert-true (fn (val msg)
  "Assert that val is #t"
  (assert-eq val true msg)))

(def assert-false (fn (val msg)
  "Assert that val is #f"
  (assert-eq val false msg)))

(def assert-list-eq (fn (actual expected msg)
  "Assert that two lists are equal (same length and elements)"
  (if (= (length actual) (length expected))
      ; Check each element - use a simple loop approach
      ; NOTE: letrec is required here because check-all calls itself recursively.
      ; A plain let would leave check-all unbound in its own body.
      (letrec ((check-all (fn (index)
        (if (>= index (length actual))
            true
            (if (if (symbol? (get expected index))
                    (eq? (get actual index) (get expected index))
                    (= (get actual index) (get expected index)))
                (check-all (+ index 1))
                (begin
                  (display "FAIL: ")
                  (display msg)
                  (display "\n  Element at index ")
                  (display index)
                  (display " differs\n  Expected: ")
                  (display (get expected index))
                  (display "\n  Actual: ")
                  (display (get actual index))
                  (display "\n")
                  (exit 1)))))))
        (check-all 0))
      (begin
        (display "FAIL: ")
        (display msg)
        (display "\n  Expected length: ")
        (display (length expected))
        (display "\n  Actual length: ")
        (display (length actual))
        (display "\n")
        (exit 1)))))

;; Alias for assert-eq (some examples use assert-equal)
(var assert-equal assert-eq)

;; Assert that a value is not nil
(def assert-not-nil (fn (val msg)
  "Assert that val is not nil"
  (if (not (eq? val nil))
      true
      (begin
        (display "FAIL: ")
        (display msg)
        (display "\n  Expected: not nil")
        (display "\n  Actual: nil")
        (display "\n")
        (exit 1)))))

;; Assert that two strings are equal
(def assert-string-eq (fn (actual expected msg)
  "Assert that two strings are equal"
  (if (= actual expected)
      true
      (begin
        (display "FAIL: ")
        (display msg)
        (display "\n  Expected: ")
        (display expected)
        (display "\n  Actual: ")
        (display actual)
        (display "\n")
        (exit 1)))))
