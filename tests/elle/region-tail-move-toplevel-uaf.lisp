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
# owning reference instead of pure-moving a borrowed upvalue) makes the nested
# case correct. This file proves that fix does NOT cover a captured TOP-LEVEL
# binding: bisected, a closure tail-passing a value bound by `(def …)` at file
# scope still pure-moves it and the callee's owned-param release over-frees it,
# while the byte-identical shape with the value bound by an enclosing `(let …)`
# is correct. The callee may even IGNORE its parameter (`sink` below returns 0) —
# the move alone is the over-free — and it is independent of whether the body is a
# native pass-through, confirming the defect is the tail-MOVE of the top-level
# reference, not anything the callee does.
#
# WHY IT IS GREEN (verified on HEAD): `tail_arg_is_borrowed`
# (src/lir/lower/control.rs) still flags an argument as borrowed only when it is a
# captured upvalue (`upvalue_bindings`); a top-level binding read inside a closure
# is resolved by a different path (a global/top-level load, not an env capture) and
# is NOT flagged, so it IS pure-moved into the owned-param callee. The over-free the
# original hypothesis predicted from that move does not happen: the top-level escape
# is increfed through the Rule 5 EscapeSite funnel, so the callee's owned-param
# release leaves the region's RC intact. docs/impl/region-rules.md Rules 5 (every escape
# increfs) and 8 (no UAF / no double-free).
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
