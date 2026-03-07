## Test Assertions Library
##
## Minimal assertion helpers for Elle tests. Self-contained, no external imports.
## Uses the `assert` primitive internally.
##
## Functions:
##   - assert-eq(actual, expected, msg)
##   - assert-true(val, msg)
##   - assert-false(val, msg)
##   - assert-list-eq(actual, expected, msg)
##   - assert-not-nil(val, msg)
##   - assert-string-eq(actual, expected, msg)
##   - assert-err(f, msg)
##   - assert-err-kind(f, expected-kind, msg)

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

## Alias for assert-eq (some tests use assert-equal)
(var assert-equal assert-eq)

## Assert that a value is not nil
(def assert-not-nil (fn (val msg)
  "Assert that val is not nil"
  (assert (not (nil? val)) msg)))

## Assert that two strings are equal
(def assert-string-eq (fn (actual expected msg)
  "Assert that two strings are equal"
  (assert (= actual expected) msg)))

## Assert that a thunk raises any error
(def assert-err (fn (f msg)
  "Assert that (f) raises an error"
  (let (([ok? _] (protect (f))))
    (assert (not ok?) msg))))

## Assert that a thunk raises an error with a specific kind keyword
(def assert-err-kind (fn (f expected-kind msg)
  "Assert that (f) raises an error with the given kind"
  (let (([ok? err] (protect (f))))
    (if ok?
      (error {:error :failed-assertion :message (-> "Expected error, got success\n" (append msg))})
      (assert-eq (get err 0) expected-kind msg)))))

(fn [] {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind})
