(elle/epoch 15)
# audited: 2026-10-26
# A module whose function answers three plain values, for the loader tests.
# docs/modules.md

(fn []
  (def test-var 42)
  (def test-string "hello")
  (def test-list (list 1 2 3))
  {:test-var test-var :test-string test-string :test-list test-list})
