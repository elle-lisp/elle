(elle/epoch 13)
# audited: 2026-09-28
# `error` raises its argument, or nil when it has none, as the value protect returns.
# tests/AGENTS.md
# docs/errors.md

(let [[ok? val] (protect (error))]
  (assert (not ok?) "error with no argument raises")
  (assert (= val nil) "error with no argument raises nil"))

(let [[ok? val] (protect (error :boom))]
  (assert (not ok?) "error with a value raises")
  (assert (= val :boom) "error with a value raises that value"))
