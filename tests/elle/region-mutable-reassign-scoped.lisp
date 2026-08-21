(elle/epoch 12)
# tests/elle/region-mutable-reassign-scoped.lisp
#
# Companion to region-toplevel-mutable-reassign.lisp. That file corners the
# FILE-LETREC (`def @x` / top-level `var`) reassign double-free. This file
# pins the cases that were ALWAYS correct — a *scoped* mutable binding
# (a `let`-bound `@x`, or a `var`/`@`-binding local to a function) whose
# region is owned by its enclosing scope, not the program root.
#
# These pass on the current tree and MUST keep passing once the file-letrec
# reassign double-free is fixed: that fix must not perturb the scoped-store
# path (different lowering — the scope region owns the value's demise, so the
# reassign needs no program-lifetime escape; verified guardfree-clean). A
# trailing top-level statement follows each block so a deferred double-free
# would surface on teardown (the load-bearing condition from the file-letrec
# repro).

# ── 1. function-local `var` reassigned to a heap pair, then read ─────────
(def f1
  (fn []
    (var x (list))
    (assign x (pair 1 2))
    (= x (pair 1 2))))
(assert (f1) "fn-local var reassigned to a heap pair reads back correctly")

# ── 2. function-local self-referential accumulation in a loop ───────────
(def f2
  (fn []
    (var acc (list))
    (each i (list 1 2 3)
      (assign acc (pair i acc)))
    (reverse acc)))
(assert (= (f2) (list 1 2 3))
        "fn-local loop self-ref accumulation preserves all elements")

# ── 3. function-local reassigned heap value ESCAPES via return ──────────
(def f3
  (fn []
    (var x (list))
    (assign x (pair 7 8))
    x))
(assert (= (f3) (pair 7 8))
        "fn-local reassigned heap value survives return (escape)")

# ── 4. top-level `let`-scoped @x reassigned to a heap value ─────────────
(let [@x (list)]
  (assign x (pair 3 4))
  (assert (= x (pair 3 4)) "top-level let @x reassigned to a heap pair OK"))

# ── 5. reassign to other heap kinds inside a function ───────────────────
(def f5
  (fn []
    (var s "")
    (assign s (concat "ab" "cd"))
    (var a (list))
    (assign a (@array 1 2 3))
    (pair s (length a))))
(assert (= (f5) (pair "abcd" 3))
        "fn-local reassign to heap string + array reads back correctly")

# ── 6. TOP-LEVEL file-letrec reassigns that are ALREADY correct ─────────
# These are file-letrec (the buggy class's scope) but involve no live heap
# value at the demise, so they are guardfree-clean today and must stay so —
# the fix must not perturb them. Aliased + recycled so a regression that
# freed the value early would surface as a wrong value (see
# region-mutable-reassign-flow.lisp for the manifestation rationale).

# 6a. immediate-only reassign — no region ever involved.
(def @ti 0)
(assign ti 1)
(assign ti 2)
(assert (= ti 2) "top-level immediate-only reassign reads back correctly")

# 6b. heap binding reassigned to an immediate — the prior heap value is
#     dropped at the overwrite; the binding then holds an immediate.
(def @th (pair 1 2))
(assign th 99)
(def junk-th (list (pair 8 8) (pair 9 9) (pair 8 8) (pair 9 9)))
(assert (= th 99) "top-level heap→immediate reassign reads back correctly")

(println "region-mutable-reassign-scoped: OK")
