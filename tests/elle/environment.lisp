# Tests for the environment primitive

(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

# (environment) was removed in the file-as-letrec model.
# Verify it signals an error.
(assert-err (fn [] (environment)) "environment is no longer available")
