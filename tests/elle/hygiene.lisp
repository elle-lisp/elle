#!/usr/bin/env elle
(elle/epoch 12)

## Macro hygiene tests — counter-factual tests that verify template symbols
## resolve to their definition-site bindings, not call-site shadows.

# ── each macro: template `rest` must resolve to the builtin ──────────

## Counter-factual: without hygiene, `rest` in the `each` template would
## resolve to the user's `& rest` parameter, causing "Cannot call" error.
(defn iterate-rest [& rest]
  (let [out @[]]
    (each item in rest
      (push out item))
    (freeze out)))

(assert (= (iterate-rest 1 2 3) [1 2 3])
        "each: template rest not captured by user rest")

## Same with `cur` — another name used internally by `each`
(defn iterate-cur [& cur]
  (let [out @[]]
    (each item in cur
      (push out item))
    (freeze out)))

(assert (= (iterate-cur "a" "b") ["a" "b"])
        "each: template cur not captured by user cur")

## Same with `seq`
(defn iterate-seq [& seq]
  (let [out @[]]
    (each item in seq
      (push out item))
    (freeze out)))

(assert (= (iterate-seq :x :y) [:x :y])
        "each: template seq not captured by user seq")

# ── when/unless: template symbols not captured ───────────────────────

(let [empty? (fn [x] true)]
  (def @reached false)
  (when true (assign reached true))
  (assert reached "when: template not captured by shadowed empty?"))

# ── Nested macro expansion ───────────────────────────────────────────

(defn collect-rest [& rest]
  (let [result @[]]
    (each x in rest
      (when (> x 0) (push result x)))
    (freeze result)))

(assert (= (collect-rest -1 2 -3 4) [2 4])
        "nested macros: each+when with shadowed rest")

# ── Struct iteration ─────────────────────────────────────────────────

(let [out @[]]
  (each [k v] in {:a 1 :b 2}
    (push out [k v]))
  (assert (= (length out) 2) "each: struct iteration"))

# ── Inbound capture (the intro-scope flip) ───────────────────────────

## A binding introduced by a macro template must not capture a free
## identifier of the same name arriving through the macro's arguments
## (src/syntax/expand/macro_expand.rs; docs/macros.md § Sets-of-Scopes).
## These were RED counterfactuals until the flip landed: pre-flip, the
## canonical case yielded 1998 by collapsing both `tmp` into one binding.

(defmacro hyg-m (expr)
  `(let [tmp 999]
     (+ tmp ,expr)))
(def tmp 7)
(assert (= (hyg-m tmp) 1006)
        "macro-introduced binding must not capture an inbound identifier")

## The template's own reference still resolves to the template's binder.
(defmacro hyg-self ()
  `(let [tmp 5]
     (* tmp tmp)))
(assert (= (hyg-self) 25) "template references resolve to template binders")

## Nested expansion: each expansion gets its own intro scope, so two
## template `tmp`s from different macros stay distinct from each other
## AND from the caller's.
(defmacro hyg-inner (e)
  `(let [tmp 100]
     (+ tmp ,e)))
(defmacro hyg-outer (e)
  `(let [tmp 10]
     (+ tmp (hyg-inner ,e))))
(assert (= (hyg-outer tmp) 117)
        "nested expansions keep three same-named bindings distinct")

## An identity macro returns its argument with use-site scopes intact
## (the pre-stamp and the flip cancel exactly on argument material).
(defmacro hyg-id (e)
  e)
(let [x 42]
  (assert (= (hyg-id x) 42) "identity macro preserves use-site resolution"))

# ── Referential transparency ─────────────────────────────────────────

## A free variable in a macro template resolves in the macro's
## definition environment, not the call site (docs/macros.md § The
## Hygiene Problem, point 2). A call-site local that shadows the name
## lacks the template reference's intro scope, so it is invisible to
## the reference — resolution falls through to the top-level binding.
(defn rt-helper [v]
  (* v 10))
(defmacro rt-use (x)
  `(rt-helper ,x))

(assert (= (rt-use 5) 50) "template reference works unshadowed")

(let [rt-helper (fn [v] :hijacked)]
  (assert (= (rt-helper 5) :hijacked) "call-site code still sees its shadow")
  (assert (= (rt-use 5) 50)
          "a call-site shadow must not capture the template's reference"))

(println "hygiene: all tests passed")
