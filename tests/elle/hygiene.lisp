#!/usr/bin/env elle
(elle/epoch 8)

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

(assert (= (iterate-rest 1 2 3) [1 2 3]) "each: template rest not captured by user rest")

## Same with `cur` — another name used internally by `each`
(defn iterate-cur [& cur]
  (let [out @[]]
    (each item in cur
      (push out item))
    (freeze out)))

(assert (= (iterate-cur "a" "b") ["a" "b"]) "each: template cur not captured by user cur")

## Same with `seq`
(defn iterate-seq [& seq]
  (let [out @[]]
    (each item in seq
      (push out item))
    (freeze out)))

(assert (= (iterate-seq :x :y) [:x :y]) "each: template seq not captured by user seq")

# ── when/unless: template symbols not captured ───────────────────────

(let [empty? (fn [x] true)]
  (def @reached false)
  (when true (assign reached true))
  (assert reached "when: template not captured by shadowed empty?"))

# ── Nested macro expansion ───────────────────────────────────────────

(defn collect-rest [& rest]
  (let [result @[]]
    (each x in rest
      (when (> x 0)
        (push result x)))
    (freeze result)))

(assert (= (collect-rest -1 2 -3 4) [2 4]) "nested macros: each+when with shadowed rest")

# ── Struct iteration ─────────────────────────────────────────────────

(let [out @[]]
  (each [k v] in {:a 1 :b 2}
    (push out [k v]))
  (assert (= (length out) 2) "each: struct iteration"))

(println "hygiene: all tests passed")
