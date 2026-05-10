(elle/epoch 10)
## Tests: phi-insertion for cond/match branch assigns
## Verifies that assigns inside branch bodies correctly propagate
## through SSA phi-insertion (not just runtime slot mutation).

## ── cond phi: single clause ──────────────────────────────────

(let [@x 0]
  (cond
    true (assign x 42))
  (assert (= x 42) "cond phi: single clause taken"))

(let [@x 0]
  (cond
    false (assign x 42))
  (assert (= x 0) "cond phi: single clause untaken"))

## ── cond phi: two clauses ────────────────────────────────────

(let [@x 0]
  (cond
    true (assign x 1)
    true (assign x 2))
  (assert (= x 1) "cond phi: first clause taken"))

(let [@x 0]
  (cond
    false (assign x 1)
    true (assign x 2))
  (assert (= x 2) "cond phi: second clause taken"))

(let [@x 0]
  (cond
    false (assign x 1)
    false (assign x 2))
  (assert (= x 0) "cond phi: no clause taken"))

## ── cond phi: with else ──────────────────────────────────────

(let [@x 0]
  (cond
    false (assign x 1)
    false (assign x 2)
    (assign x 3))
  (assert (= x 3) "cond phi: else taken"))

(let [@x 0]
  (cond
    true (assign x 1)
    false (assign x 2)
    (assign x 3))
  (assert (= x 1) "cond phi: first clause, else skipped"))

## ── cond phi: condition with side effects ────────────────────
## The condition function must be called the right number of times.

(def @side-effect-count 0)
(defn counting-pred [val expected]
  (assign side-effect-count (+ side-effect-count 1))
  (= val expected))

(let [@x 0]
  (cond
    (counting-pred 1 2) (assign x 1)
    (counting-pred 1 1) (assign x 2)
    (assign x 3))
  (assert (= x 2) "cond phi: correct branch value")
  (assert (= side-effect-count 2) "cond phi: conditions evaluated correctly"))

## ── cond phi: assign inside begin ────────────────────────────

(let [@x 0]
  (cond
    true (begin
           (assert true "side-effect")
           (assign x 99)))
  (assert (= x 99) "cond phi: assign inside begin"))

## ── cond phi: multiple assigned bindings ─────────────────────

(let [@x 0
      @y 0]
  (cond
    false (begin
            (assign x 1)
            (assign y 10))
    true (begin
           (assign x 2)
           (assign y 20)))
  (assert (= x 2) "cond phi: multi-binding x")
  (assert (= y 20) "cond phi: multi-binding y"))

## ── cond phi: partial assigns ────────────────────────────────
## Some branches assign x, others don't

(let [@x 0]
  (cond
    false nil
    true (assign x 42)
    (assign x 99))
  (assert (= x 42) "cond phi: partial assign"))

(let [@x 0]
  (cond
    false (assign x 1)
    false nil)
  (assert (= x 0) "cond phi: partial, none taken"))

## ── match phi ────────────────────────────────────────────────

(let [@x 0]
  (match 2
    1 (assign x 10)
    2 (assign x 20)
    _ (assign x 30))
  (assert (= x 20) "match phi: second arm"))

(let [@x 0]
  (match :a
    :b (assign x 1)
    :a (assign x 2)
    _ nil)
  (assert (= x 2) "match phi: keyword arm"))

(let [@x 0]
  (match 99
    1 (assign x 1)
    2 (assign x 2)
    _ nil)
  (assert (= x 0) "match phi: default no assign"))

## ── cond phi: use after multiple conds ───────────────────────

(let [@x 0]
  (cond
    true (assign x 1))
  (assert (= x 1) "cond phi chain: first")
  (cond
    true (assign x (+ x 10)))
  (assert (= x 11) "cond phi chain: second"))

## ── cond phi inside function ─────────────────────────────────

(defn classify [n]
  (let [@result :unknown]
    (cond
      (< n 0) (assign result :negative)
      (= n 0) (assign result :zero)
      (assign result :positive))
    result))

(assert (= (classify -5) :negative) "cond phi fn: negative")
(assert (= (classify 0) :zero) "cond phi fn: zero")
(assert (= (classify 5) :positive) "cond phi fn: positive")

## ── cond phi: assign only in some clauses ─────────────────────

(let [@x 10
      @y 20]
  (cond
    false (assign x 1)
    true nil)
  (assert (= x 10) "cond phi: x unchanged when clause didn't assign")
  (assert (= y 20) "cond phi: y unchanged when no clause assigns it"))

## ── cond phi: three clauses, middle taken ─────────────────────

(let [@x 0]
  (cond
    false (assign x 1)
    true (assign x 2)
    false (assign x 3))
  (assert (= x 2) "cond phi: three clauses, middle taken"))

## ── match phi: with begin ─────────────────────────────────────

(let [@x 0]
  (match :b
    :a (begin
         (assign x 10)
         nil)
    :b (begin
         (assign x 20)
         nil)
    _ nil)
  (assert (= x 20) "match phi: begin in arm"))

## ── match phi: multiple bindings ──────────────────────────────

(let [@x 0
      @y 0]
  (match 2
    1 (begin
        (assign x 10)
        (assign y 100))
    2 (begin
        (assign x 20)
        (assign y 200))
    _ nil)
  (assert (= x 20) "match phi: multi-binding x")
  (assert (= y 200) "match phi: multi-binding y"))

## ── cond phi: sequential conds ────────────────────────────────

(defn multi-cond [n]
  (let [@x 0
        @y 0]
    (cond
      (> n 0) (assign x 1))
    (cond
      (> n 5) (assign y 1))
    (+ x y)))

(assert (= (multi-cond -1) 0) "sequential conds: both false")
(assert (= (multi-cond 3) 1) "sequential conds: first true")
(assert (= (multi-cond 10) 2) "sequential conds: both true")

(println "branch-phi: all tests passed")
