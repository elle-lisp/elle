(elle/epoch 12)
# Regression guard (GREEN on HEAD): a closure that reads a TOP-LEVEL (file-scope)
# binding and passes it as a TAIL-CALL ARGUMENT must not over-free that binding's
# region. The failure mode this guards against is a double-free / use-after-free —
# a closure called repeatedly draining the binding's region to zero then freeing
# it again (regionstore phantom/double-free abort without guardfree, SIGSEGV under
# guardfree). That fault does NOT occur on this tree (see the status note at the
# bottom): this is a guard against regression, not a live RED witness.
#
# RELATION TO region-tail-move-borrow-uaf. That test pins the same hazard for a
# value captured BY VALUE into a closure from an ENCLOSING let — and its fix
# (`tail_arg_is_borrowed` in src/lir/lower/control.rs hands the callee one fresh
# owning reference instead of pure-moving a borrowed upvalue) covers this file's
# shape by the SAME route. The callee may even IGNORE its parameter (`sink` below
# returns 0) — the move alone is the hazard — and it is independent of whether the
# body is a native pass-through.
#
# WHY IT IS GREEN (verified by disassembly, --trace=bytecode): a RUNTIME-valued
# top-level binding like `(def s (list "z"))` is NOT a compile-time constant, so
# the closure CAPTURES it (the lambda's proto shows the capture and the
# `IncrefValueRegion` retain right before the `TailCall`) — the upvalue borrow
# route of `tail_arg_is_borrowed`, same as the enclosing-let shape. Only a
# compile-time-CONSTANT heap value (a stdlib-export closure — `immutable_values`)
# skips the capture and reads as `LoadConst`; that route is the CONST borrow arm,
# pinned separately by region-const-tail-move-borrow-uaf.lisp. docs/impl/region/
# rules.md Rule 5 (the borrowed tail-call argument escape site).
#
# STATUS: GREEN on the vm and jit tiers, including under `--trace=guardfree` (the
# robust oracle, pinned as a subprocess in tests/integration/elle_scripts.rs). A
# regression that drops the top-level escape incref would drain the binding's
# region mid-loop and crash here — that is what this guard catches.

# ── subject ───────────────────────────────────────────────────────
# `sink` ignores its arg and returns an immediate, so the ONLY thing that can be
# freed is the MOVED arg's region — the over-free of the top-level `s`.
(defn sink [v]
  0)

# `s` is a TOP-LEVEL binding; the closure `(fn () (sink s))` reads it and tail-
# passes it. The control in region-tail-move-borrow-uaf binds the analogue in an
# enclosing `let` and is correct — here `s` is file-scope.
(def s (list "z"))

(var i 0)
(while (%lt i 500)
  ((fn () (sink s)))
  (assign i (%add i 1)))

# The top-level binding must still be live and correct after 500 tail-moves.
# Regression: the region is over-freed mid-loop (crash) or this read faults.
# Correct (current): returns "z".
(assert (= (first s) "z")
        "a top-level binding tail-moved into an owned-param callee was over-released")

(println "region-tail-move-toplevel-uaf: ok")
