(elle/epoch 12)
# Fixture: a form that FAILS, to exercise the assert-macro payload capture.
# Per docs/test-runner.md the macro must record, on failure:
#   :message "wrong sum"   -> becomes the form's derived label
#   :value   false         -> the predicate's evaluated result
#   :syntax  (= (+ 1 1) 3) -> the predicate, unevaluated, as data
# and for a recognized comparison (= LHS RHS) the runner fills
#   actual   = LHS value (2), expected = RHS value (3).
(assert (= (+ 1 1) 3) "wrong sum")
