(elle/epoch 15)
# audited: 2026-11-09
# A read of a letrec or body binding before its initializer runs is an error, unless a body's read finds a binding outside the body.
# docs/bindings.md
#
# The counter-factual: a read of a binding's value before its own initializer
# ran yielded nil, which hides the mistake. A function body's direct read now
# finds the binding of the name outside the body, and is an error only where
# none exists.

# ── subjects: use before initialization is an error ───────────────
(let [[ok res] (protect (eval (quote (letrec [a b
                                              b 7]
                                       a))))]
  (assert (not ok) "use-before-init in letrec must error, not return a value"))

(let [[ok res] (protect (eval (quote ((fn []
                                        (def a b)
                                        (def b 7)
                                        a)))))]
  (assert (not ok)
          "use-before-init in a fn body with no outer binding must error"))

# ── subject: a body's read before the def finds the outer binding ─
(let [[ok res] (protect (eval (quote ((fn [b]
                                        (def a b)
                                        (def b 7)
                                        [a b]) 3))))]
  (assert ok "a direct read before the body's def finds the parameter")
  (assert (= res [3 7]) "the read before the def, the read after it"))

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
