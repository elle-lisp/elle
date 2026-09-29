(elle/epoch 12)
# Counterfactual: referencing a `letrec*` binding's VALUE before its own
# initializer has run must be an ERROR, not a silent `nil`.
#
# Elle's `letrec` is `letrec*` (left-to-right init; docs/bindings.md), and a
# function body and the file top-level are `letrec*` contexts too. In all of
# them, a forward reference to a not-yet-initialized binding's value is a
# mistake. Today it yields `nil` silently — a least-surprise violation that
# hides real bugs (you read a binding before it exists and get `nil` instead of
# a diagnostic). docs/bindings.md (letrec — recursive bindings).
#
# RED now: the subjects return nil (no error), so `(protect …)` reports ok=true.
# GREEN once use-before-init raises "'b' referenced before its initialization".
#
# The CONTROLS must keep passing — they are the legitimate `letrec*` uses that
# the fix must NOT break: a backward value dependency, and a forward reference
# *through a lambda* (deferred until after all initializers run).

# ── subjects: use-before-init must error ──────────────────────────
(let [[ok res] (protect (eval (quote (letrec [a b
                                       b 7]
                                       a))))]
  (println "ubi letrec   ok?=" ok " res=" res)
  (assert (not ok) "use-before-init in letrec must error, not return a value"))

(let [[ok res] (protect (eval (quote ((fn []
                                        (def a b)
                                        (def b 7)
                                        a)))))]
  (println "ubi fn-body  ok?=" ok " res=" res)
  (assert (not ok)
          "use-before-init in a fn body (letrec*) must error, not return a value"))

# ── controls: legitimate letrec* uses must still succeed ──────────
(let [[ok res] (protect (eval (quote (letrec [a 1
                                       b (%add a 1)]
                                       b))))]
  (assert ok "control: backward value dependency must succeed")
  (assert (= res 2) "control: b sees a's value (left-to-right init)"))

(let [[ok res] (protect (eval (quote (letrec [a (fn [] b)
                                       b 7]
                                       (a)))))]
  (assert ok "control: forward ref through a lambda must succeed")
  (assert (= res 7) "control: lambda defers the use of b until after init"))

(println "binding-use-before-init: ok")
