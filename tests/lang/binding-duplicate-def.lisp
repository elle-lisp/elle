(elle/epoch 15)
# audited: 2026-11-09
# An explicit letrec rejects a duplicate binder by binding identity, and a function body may define a name again.
# docs/bindings.md
#
# The counter-factual: a function body rejected a second def of a name as a
# duplicate, as an explicit letrec rejects a second binder. A body now makes a
# new binding at each def, which code after it reads.

# ── subject: a function body redefines a name ─────────────────────────
(let [[ok res] (protect (eval (quote ((fn []
                                        (def x 1)
                                        (def x 2)
                                        x)))))]
  (assert ok "a second def in a function body compiles")
  (assert (= res 2) "code after the second def reads it"))

# ── control: explicit letrec rejects a duplicate binder ───────────────
(let [[ok res] (protect (eval (quote (letrec [x 1
                                              x 2]
                                       x))))]
  (assert (not ok) "control: explicit letrec rejects a duplicate binder"))

# ── control: hygiene — macro names are distinct identities (letrec) ──
# Duplicates are judged by binding identity (name + hygiene scopes), not
# spelling (docs/bindings.md, docs/macros.md): a macro template's `x` and
# a user's `x` in one letrec are two bindings, and each side's references
# resolve to its own.
# (File-level, not eval: a macro DEFINED inside an eval'd form loses its
# template scopes in the eval expander, so under eval the two binders share
# one identity and the duplicate error is correct there.)
(defmacro letrec-mixed [user-name user-init]
  `(letrec [x 1
            ,user-name ,user-init]
     (%add x ,user-name)))

(let [r (letrec-mixed x 2)]
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
