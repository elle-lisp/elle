(elle/epoch 12)
# audited: 2026-09-29
# A compile-time-constant closure passed as a tail-call argument keeps its region, and the retain it takes does not leak.
# docs/impl/region/rules.md
#
# The sidecar arms guardfree, so a read of the freed constant faults.
#
# region-tail-move-borrow-uaf.lisp covers the captured upvalue and
# region-or-tail-move-borrow-uaf.lisp the branch; this file covers the
# constant. The owned-params calling convention (Rule 5) makes a tail-call
# arg a MOVE: the caller emits no incref, and the owned-param callee releases
# the arg at its last use. A binding with a compile-time-known value — a
# stdlib export like `+`/`inc`/`map`, a primitive's closure value, a
# `begin-for-syntax` value — is NOT captured: the lowerer emits `LoadConst`
# from `immutable_values`, so the frame holds no reference to it at all.
# `arg_leaf_is_borrowed` (src/lir/lower/control.rs) therefore treats a
# constant HEAP value as borrowed, and the caller hands the callee one fresh
# reference (`IncrefValueRegion`) for its release to consume. An immediate
# constant (int, keyword, native-fn) has no region and needs no retain.
#
# The counter-factual: moving the constant hands the callee a reference the
# caller never owned. Each call is a net -1 on the constant's region until it
# frees under the stdlib env, and the next read faults. The everyday idiom
#
#   (defn incs [xs] (map inc xs))
#
# tail-calls `map` with the constant closure `inc`, and a handful of calls
# drains it. region-fold-closure-arg-uaf.lisp reaches the same shape through
# a driver thunk's tail call that passes the constant `+`.

# ── subjects ──────────────────────────────────────────────────────
# `sink` ignores its arg and returns an immediate, so the ONLY reference the
# callee releases is the moved arg's — the over-free under test, isolated from
# anything the callee does with the value.
(defn sink [v]
  0)

# ── witness (a): minimal — const closure straight into an ignoring callee ──
# Each iteration tail-moves the CONST `inc` into `sink`. A stdlib closure's
# region rc is small (single digits); 500 iterations drain it to a free within
# the first handful, and a later use faults. Correct: rc is untouched.
(def @i 0)
(while (%lt i 500)
  ((fn [] (sink inc)))
  (assign i (%add i 1)))
(assert (= (inc 41) 42)
        "const closure freed by tail-moves into an ignoring callee")

# ── witness (b): the everyday HOF idiom ──
# `(map inc xs)` in tail position of a user fn: BOTH `map` (the callee is read
# from the const table too — but as the CALLEE it is not moved) and `inc` (the
# arg — moved) are compile-time constants. Only the arg is at risk.
(defn incs [xs]
  (map inc xs))
(def @j 0)
(while (%lt j 500)
  (incs [1 2 3])
  (assign j (%add j 1)))
(assert (= (incs [1 2]) [2 3])
        "stdlib closure freed by the (defn f [xs] (map inc xs)) idiom")

# ── witness (c): balance — the retain must not leak ──
# The fresh owning reference handed per call must be CONSUMED by the callee's
# release: net region growth across 500 calls stays bounded (a leaked retain
# would pin `inc`'s region rc up by 500 and show as monotone region growth in
# the arena census; the drained case is caught by (a)/(b) faulting).
(def before (arena/region-count))
(def @k 0)
(while (%lt k 500)
  ((fn [] (sink inc)))
  (assign k (%add k 1)))
(assert (%lt (%sub (arena/region-count) before) 50)
        "the const tail-arg retain leaks (regions grow per call)")

(println "region-const-tail-move-borrow-uaf: ok")
