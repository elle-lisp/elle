(elle/epoch 12)
# RED counterfactual (deferred) — lexical scoping: a `let` binding shadows
# ANY outer binding of the same name, stdlib operators included.
#
# Asserts the SPEC-correct behavior, so it FAILS until the defect is
# fixed (the branch merges only when every known defect is green).
# GREEN WHEN the intrinsic rewrite is guarded on binding identity.
#
# WHAT IT REPRODUCES
#   (let [+ -] (+ 5 5)) yields 10, not 0: the intrinsic-specialization
#   pass (`rewrite_calls`, src/hir/typeinfer.rs) rewrites calls to %add
#   keyed on the callee BINDING'S NAME without checking that the binding
#   is the stdlib one. A user binding that happens to be named `+` is
#   also named `+`, so the rewrite fires and the user's binding is
#   silently ignored. Resolution itself is correct; the by-name
#   specialization clobbers it afterward.
#
# FIX DIRECTION
#   Guard the rewrite on the binding's identity, not its spelling —
#   `BindingInner::is_primitive` records exactly "injected by
#   bind_primitives", so `rewrite_calls` should require it before
#   treating a call as the stdlib operator.

(assert (= (let [+ -]
             (+ 5 5)) 0)
        "a local binding named + must shadow the stdlib + like any other binding")

(println "operator-shadowing: OK")
