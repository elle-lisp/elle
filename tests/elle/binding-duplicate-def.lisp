(elle/epoch 12)
# Counterfactual: defining the same name twice in a `letrec*` context (a lambda
# body, the file top-level) must be a duplicate-definition ERROR — the same rule
# an explicit `letrec` already enforces.
#
# Duplicate definition has no coherent meaning in a recursive context: a forward
# reference binds the FIRST definition while a later reference binds the SECOND
# (the file-letrec footgun — redefine a function and existing callers silently
# keep the old one). docs/bindings.md (Top-level implicit letrec).
#
# Today: explicit `(letrec [x 1 x 2] x)` already errors (GREEN control), but a
# lambda body silently allows it (`((fn [] (def x 1) (def x 2) x))` => 2). That
# asymmetry is the bug. RED now: the lambda-body subject reports ok=true.
# GREEN once the implicit `letrec*` contexts reject duplicates like the explicit
# one. The fix must NOT ban sequential shadowing (`let`/`do`) — that is
# refinement, not redefinition, and stays legal (controls below).

# ── subject: duplicate def in a lambda body (letrec*) must error ──
(let [[ok res] (protect (eval (quote ((fn []
                                        (def x 1)
                                        (def x 2)
                                        x)))))]
  (println "dup fn-body  ok?=" ok " res=" res)
  (assert (not ok)
          "duplicate def in a lambda body (letrec*) must error, not return a value"))

# ── control: explicit letrec already rejects duplicates (the target) ──
(let [[ok res] (protect (eval (quote (letrec [x 1
                                       x 2]
                                       x))))]
  (assert (not ok)
          "control: explicit letrec must reject duplicate binding (already does)"))

# ── control: hygiene — macro names are distinct identities (letrec) ──
# Duplicates are judged by binding identity (name + hygiene scopes), not
# spelling (docs/bindings.md, docs/macros.md): a macro template's `x` and
# a user's `x` in one letrec are two bindings, and each side's references
# resolve to its own. RED while the duplicate check keys on bare names.
# (File-level, not eval: a macro DEFINED inside an eval'd form loses its
# template scopes in the eval expander — a separate, pre-existing defect —
# so under eval the two binders genuinely share one identity and the
# duplicate error is correct there.)
(defmacro letrec-mixed [user-name user-init]
  `(letrec [x 1
            ,user-name ,user-init]
     (%add x ,user-name)))

(let [r (letrec-mixed x 2)]
  (println "hygiene letrec =" r)
  (assert (= r 3)
          "control: macro letrec binding must not collide with a user binding"))

# ── control: hygiene in a lambda body — macro def vs user def ──
(defmacro def-hidden []
  `(def hidden 1))

(let [r ((fn []
           (def-hidden)
           (def hidden 2)
           hidden))]
  (assert (= r 2)
          "control: a macro-introduced define must not collide with a user define"))

# ── controls: sequential shadowing (refinement) must stay legal ───
(let [[ok res] (protect (eval (quote (let [x 1]
                                       (let [x (%add x 1)]
                                         (let [x (%mul x 10)]
                                           x))))))]
  (assert ok "control: nested-let shadowing must succeed")
  (assert (= res 20) "control: refinement by shadowing is not redefinition"))

(println "binding-duplicate-def: ok")
