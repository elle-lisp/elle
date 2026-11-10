(elle/epoch 15)
# audited: 2026-10-26
# A module whose function builds a fresh counter on each call, for the tests that load it twice.
# docs/modules.md

(fn []
  (var count 0)
  (defn inc []
    (assign count (+ count 1))
    count)
  {:inc inc :count (fn [] count)})
