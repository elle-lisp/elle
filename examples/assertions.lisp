## Elle Assertions Library
##
## Shared assertion helpers for all examples. Load this file with:
##   (import-file "./examples/assertions.lisp")
##
## Functions:
##   - assert-eq(actual, expected, msg)
##     Assert that actual equals expected (using =, which is numeric-aware)
##   - assert-equal(actual, expected, msg)
##     Alias for assert-eq
##   - assert-true(val, msg)
##     Assert that val is true
##   - assert-false(val, msg)
##     Assert that val is false
##   - assert-list-eq(actual, expected, msg)
##     Assert that two lists are equal (same length and elements)
##   - assert-not-nil(val, msg)
##     Assert that val is not nil
##   - assert-string-eq(actual, expected, msg)
##     Assert that two strings are equal
##   - assert-err(f, msg)
##     Assert that thunk f signals any error
##   - assert-err-kind(f, expected-kind, msg)
##     Assert that thunk f signals an error with the given kind keyword
##
## All assertions signal errors on failure, making examples
## act as contracts for the implementation.

(def assert-eq (fn (actual expected msg)
  "Assert that actual equals expected"
  (if (= actual expected)
      true
      (error {:error :failed-assertion :message (-> "Expected: "
                                                    (append (string expected))
                                                    (append "\nActual: ")
                                                    (append (string actual))
                                                    (append "\n")
                                                    (append msg))}))))

(def assert-true (fn (val msg)
  "Assert that val is true"
  (assert val msg)))

(def assert-false (fn (val msg)
  "Assert that val is false"
  (assert (not val) msg)))

(def assert-list-eq (fn (actual expected msg)
  "Assert that two lists are equal (same length and elements)"
  (if (= (length actual) (length expected))
      (letrec ((check-all (fn (index)
        (if (>= index (length actual))
            true
            (if (= (get actual index) (get expected index))
                (check-all (+ index 1))
                (error {:error :failed-assertion :message (-> "Element at index "
                                                             (append (string index))
                                                             (append " differs\nExpected: ")
                                                             (append (string (get expected index)))
                                                             (append "\nActual: ")
                                                             (append (string (get actual index)))
                                                             (append "\n")
                                                             (append msg))}))))))
        (check-all 0))
      (error {:error :failed-assertion :message (-> "Expected length: "
                                                   (append (string (length expected)))
                                                   (append "\nActual length: ")
                                                   (append (string (length actual)))
                                                   (append "\n")
                                                   (append msg))}))))

## Alias for assert-eq (some examples use assert-equal)
(var assert-equal assert-eq)

## Assert that a value is not nil
(def assert-not-nil (fn (val msg)
  "Assert that val is not nil"
  (assert (not (nil? val)) msg)))

## Assert that two strings are equal
(def assert-string-eq (fn (actual expected msg)
  "Assert that two strings are equal"
  (assert (= actual expected) msg)))

## Assert that a thunk signals any error
(def assert-err (fn (f msg)
  "Assert that (f) signals an error"
  (let (([ok? _] (protect (f))))
    (assert (not ok?) msg))))

## Assert that a thunk signals an error with a specific kind keyword
(def assert-err-kind (fn (f expected-kind msg)
  "Assert that (f) signals an error with the given kind"
  (let (([ok? err] (protect (f))))
    (if ok?
      (begin
        (display "FAIL: ")
        (display msg)
        (display "\n  Expected error, got success\n")
        (exit 1))
      (assert-eq (get err 0) expected-kind msg)))))

## Module exports
(fn [] {:assert-eq assert-eq :assert-equal assert-equal :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind})
