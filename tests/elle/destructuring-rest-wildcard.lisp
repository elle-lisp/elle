(elle/epoch 12)
# audited: 2026-09-16
# What a `& _` rest still owes.
#
# `[a & _]` is how a destructure says "ignore the rest", and nothing in the
# program can read the collection such a rest would hold. So the compiler builds
# none — which is invisible from here, and would be a fine thing to get wrong
# quietly.
#
# THE TRAP. The rest of an array pattern CHECKS that the scrutinee is an array
# as well as collecting what is left of it, so a pattern that ignores the rest
# must not lose the check with it. A fixed element makes the same check; a
# pattern with no fixed element — `[& _]` — has only the rest.
#
# THE COUNTER-FACTUAL. A struct rest checks nothing on any input, which is why
# `{& _}` is asserted to bind rather than to signal. Reading the two families as
# one would make this file demand an error the language has never raised.
#
# tests/elle/destructuring.lisp covers destructuring at large; this file is the
# one shape whose lowering turns on what nothing can read.

(defn assert-err [thunk msg]
  (let [[ok? _] (protect (thunk))]
    (assert (not ok?) msg)))

# ── the fixed names are still bound ───────────────────────────────────────────

(begin
  (def [aw & _] [1 2 3])
  (assert (= aw 1) "array rest wildcard: fixed element"))

(begin
  (def [dw ew & _] [1 2 3 4])
  (assert (= (+ dw ew) 3) "array rest wildcard: two fixed elements"))

(begin
  (def (bw & _) (list 1 2 3))
  (assert (= bw 1) "list rest wildcard: fixed element"))

(begin
  (def {:a cw & _} {:a 1 :b 2 :c 3})
  (assert (= cw 1) "struct rest wildcard: named key"))

(begin
  (def @[mw & _] @[7 8 9])
  (assert (= mw 7) "@array rest wildcard: fixed element"))

(assert (= (let [[x & _] [1 2 3]]
             x) 1) "array rest wildcard in let")
(assert (= ((fn [[x & _]] x) [1 2 3]) 1) "array rest wildcard in a parameter")
(assert (= (match [1 2 3]
             [x & _] x) 1) "array rest wildcard in match")

# ── the check the rest also makes ─────────────────────────────────────────────

(assert-err (fn ()
              (let [[fw & _] 5]
                fw)) "array rest wildcard: a non-array still signals")
(assert-err (fn ()
              (let [[& _] 5]
                0)) "bare rest wildcard: a non-array still signals")

# A struct rest signals on nothing, so neither of these is an error.
(assert (= (let [{& _} 5]
             :ok) :ok) "struct rest wildcard: a non-struct binds")
(assert (= (let [[] 5]
             :ok) :ok) "an empty array pattern checks nothing")

(println "destructuring-rest-wildcard: ok")
