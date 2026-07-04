(elle/epoch 12)
# RED counterfactual (deferred) — referential transparency: a free variable
# in a macro template must resolve in the macro's DEFINITION environment,
# not the call site's (docs/macros.md § The Hygiene Problem, point 2).
#
# Asserts the SPEC-correct behavior, so it FAILS until the defect is
# fixed (the branch merges only when every known defect is green).
# GREEN WHEN a per-defmacro definition scope lands; then fold this file
# into tests/elle/hygiene.lisp.
#
# WHAT IT REPRODUCES
#   The intro-scope flip protects BINDERS (inbound capture), but every
#   top-level form carries the same universal prelude scope, so a
#   template's reference to `rt-helper` and a call-site shadow of
#   `rt-helper` have identical scope sets — and the innermost binding
#   wins. HEAD yields :hijacked instead of 50.
#
# FIX DIRECTION
#   A per-defmacro DEFINITION scope: fresh-minted when the defmacro form
#   is expanded, stamped on the macro's template (the quasiquote
#   SyntaxLiteral symbols already preserve their scope sets through the
#   transformer, so they would carry it) and recorded on the
#   definition-site bindings the template can see. Template references
#   then out-rank call-site shadows under the subset rule. Fully
#   compatible with the intro-scope flip.

(defn rt-helper [v]
  (* v 10))
(defmacro rt-use (x)
  `(rt-helper ,x))

(assert (= (rt-use 5) 50) "template reference works unshadowed")

(let [rt-helper (fn [v] :hijacked)]
  (assert (= (rt-use 5) 50)
          "a call-site shadow must not capture the template's reference"))

(println "hygiene-definition-scope: OK")
