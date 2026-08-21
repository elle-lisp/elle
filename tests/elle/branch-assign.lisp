(elle/epoch 12)
## Tests: assign inside branch bodies (cond, match)
## Verifies that (assign x val) inside begin blocks within
## branch arms correctly mutates the outer binding.

## ── cond + begin ──────────────────────────────────────────

(let [@x false]
  (cond
    true (begin
           (assign x 99)))
  (assert (= x 99) "cond single-clause begin assign"))

(let [@x false]
  (cond
    false 0
    true (begin
           (assign x 99)))
  (assert (= x 99) "cond two-clause begin assign"))

(let [@x false]
  (cond
    false 0
    false 1
    true (begin
           (assign x 99)))
  (assert (= x 99) "cond three-clause begin assign"))

## cond with default (odd trailing element)
(let [@x false]
  (cond
    false 0
    (begin
      (assign x 99)))
  (assert (= x 99) "cond default begin assign"))

## cond where the taken branch does NOT assign
(let [@x 42]
  (cond
    true nil
    false (begin
            (assign x 0)))
  (assert (= x 42) "cond untaken branch preserves"))

## ── match + begin ─────────────────────────────────────────

(let [@x false]
  (match 1
    1 (begin
        (assign x 99))
    _ nil)
  (assert (= x 99) "match begin assign"))

(let [@x false]
  (match :a
    :b (begin
         (assign x 1))
    :a (begin
         (assign x 2))
    _ nil)
  (assert (= x 2) "match second-arm begin assign"))

## match where no arm assigns
(let [@x 42]
  (match 1
    1 "one"
    _ "other")
  (assert (= x 42) "match preserves when no assign"))

## ── nested ────────────────────────────────────────────────

(let [@x 0]
  (cond
    true
      (begin
        (cond
          true (begin
                 (assign x 1)))
        (assert (= x 1) "nested cond begin assign inner")
        (assign x (+ x 10))))
  (assert (= x 11) "nested cond begin assign outer"))

(println "branch-assign: all tests passed")
