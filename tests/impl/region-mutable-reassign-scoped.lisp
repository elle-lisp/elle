(elle/epoch 12)
# audited: 2026-09-29
# A scoped mutable binding reads back its reassigned heap value, and so does a top-level one with no live heap value at its demise.
# docs/impl/region/bindings.md
#
# The sidecar arms guardfree, so a read of a freed value faults.
#
# region-toplevel-mutable-reassign.lisp covers the FILE-LETREC binding
# (`def @x`, a top-level `var`). This file covers the *scoped* mutable binding:
# a `let`-bound `@x`, or a `var` or `@`-binding local to a function, whose
# value's demise its enclosing scope owns rather than the program root. The
# scoped path needs no program-lifetime escape. A trailing top-level statement
# follows each block, so a deferred double-free surfaces at teardown.

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

# ── 6. TOP-LEVEL file-letrec reassigns with no live heap value ──────────
# These are file-letrec bindings, but no live heap value reaches the demise.
# Aliased and recycled, so a value freed early reads back wrong
# (region-mutable-reassign-flow.lisp explains the recycling).

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
