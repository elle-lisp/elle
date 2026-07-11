(elle/epoch 12)
# intrinsic-mutable-literal.lisp — a mutable-collection literal's type is proven,
# so it satisfies a %-store op's container contract (docs/intrinsics.md § The
# contract: prove or reject).
#
# `@"..."` desugars to `(%thaw "...")`; `%thaw`/`%freeze` type their result as
# the mutable/immutable twin of the operand, so a mutable-string literal proves
# as @string just as `@[]` proves as @array. Before that typing, `@""` inferred
# Top and every `(%string-push @"" …)` — any mutable-string literal flowing into
# a %-store op — was rejected at compile time. This file compiling AT ALL is the
# counterfactual: each form below is a %-op in call position whose container is a
# mutable literal.

# ── Mutable-string literal proves as @string ───────────────────────────────
(def @s @"")
(%string-push s "ab")
(%string-push s "cd")
(assert (= (freeze s) "abcd") "mutable-string literal feeds %string-push")

# A mutable-string literal built inline (the desugar's direct result) as the
# container. `length` (the wrapper) reads the result — the push RESULT type is a
# separate concern; this asserts the literal CONTAINER proved.
(def @inl @"xy")
(%string-push inl "z")
(assert (= (length inl) 3) "inline @\"…\" literal feeds %string-push")

# Through a proven-container-typed direct call.
(defn spush [buf suffix]
  (%string-push buf suffix))
(assert (= (freeze (spush @"hi" "!")) "hi!")
        "mutable-string literal proves as a directly-called param")

# ── Sibling mutable literals (already proven, guarded here for parity) ──────
(def @a @[])
(%array-push a 1)
(%array-push a 2)
(assert (= (freeze a) [1 2]) "mutable-array literal feeds %array-push")

# ── %freeze / %thaw type as the operand's twin ─────────────────────────────
# %freeze of a mutable literal proves as the immutable container, feeding %get.
(assert (= (%get (%freeze @[7 8 9]) 1) 8)
        "%freeze of @[…] proves as array, feeds %get")
# %thaw of an immutable literal proves as the mutable container, feeding a store.
(def @t (%thaw "ab"))
(%string-push t "c")
(assert (= (freeze t) "abc") "%thaw of a string literal proves as @string")

(println "intrinsic-mutable-literal: all tests passed")
