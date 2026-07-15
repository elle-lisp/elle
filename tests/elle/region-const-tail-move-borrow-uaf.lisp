(elle/epoch 12)
# Counterfactual: a COMPILE-TIME-CONSTANT heap value (a stdlib-export closure)
# passed as a TAIL-CALL ARGUMENT to an owned-param callee is over-released —
# use-after-free. The CONST sibling of region-tail-move-borrow-uaf.lisp
# (captured upvalue) and region-or-tail-move-borrow-uaf.lisp (branch/phi).
#
# The owned-params calling convention (docs/impl/region/rules.md Rule 5) makes a
# tail-call arg a pure MOVE: the caller emits no incref, and the owned-param
# callee releases the arg at its last use. That is sound ONLY when the caller
# owns a transferable reference. A binding with a compile-time-known value —
# a stdlib export like `+`/`inc`/`map`, a primitive's closure value, a
# `begin-for-syntax` value — is deliberately NOT captured (the lowerer emits
# `LoadConst` from `immutable_values`; hir/analyze/scopes.rs skips the capture),
# so the frame holds NO reference to it at all: the owning references belong to
# the stdlib env that seeded it. Pure-moving it hands the callee a reference
# the caller never owned; each call is a net -1 on the constant's region until
# it frees UNDER the stdlib env, and the next read faults (tag/object mismatch
# panic on plain VM, SIGSEGV under --trace=guardfree, the robust oracle).
#
# This is as user-reachable as a leak class gets — the everyday HOF idiom
#
#   (defn incs [xs] (map inc xs))
#
# tail-calls `map` with the CONST closure `inc`; a handful of `(incs …)` calls
# drains `inc`'s region rc to a free. The fold shape that first exposed it
# (tests/integration/fixtures/region-fold-closure-arg-uaf.lisp) is the same
# hole reached through a driver thunk `(fn [] (fold-threaded + 0 …))` — the
# threading/recursion in that fixture was never the mechanism (a ZERO-iteration
# callee still drains 1/call); the hole is the thunk's own tail call passing
# the const `+`.
#
# GREEN since `arg_leaf_is_borrowed` (src/lir/lower/control.rs) treats a
# compile-time-constant HEAP value as borrowed: the caller hands the callee one
# fresh owning reference (`IncrefValueRegion`), which the callee's owned-param
# release consumes — the stdlib env's references stay intact. An immediate
# constant (int, keyword, native-fn) has no region and needs no retain.

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
