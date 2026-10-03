(elle/epoch 13)
# audited: 2026-09-29
# A closure that tail-passes a top-level binding hands the callee a fresh reference, so the binding survives every call.
# docs/impl/region/rules.md
#
# The sidecar arms guardfree.
#
# A closure that reads a TOP-LEVEL (file-scope) binding and passes it as a
# TAIL-CALL ARGUMENT must not over-free that binding's region. The
# counter-factual drains the region to zero over repeated calls and frees it
# again: a regionstore phantom/double-free abort without guardfree, a SIGSEGV
# under it.
#
# RELATION TO region-tail-move-borrow-uaf.lisp. That test pins the same hazard
# for a value captured BY VALUE from an ENCLOSING let, and one route covers both:
# `tail_arg_is_borrowed` (src/lir/lower/control.rs) hands the callee one fresh
# owning reference instead of pure-moving a borrowed upvalue. The callee may
# IGNORE its parameter (`sink` below returns 0) — the move alone is the hazard —
# and it is independent of whether the body is a native pass-through.
#
# WHY THE UPVALUE ROUTE (read off `--trace=bytecode`): a RUNTIME-valued top-level
# binding like `(def s (list "z"))` is not a compile-time constant, so the
# closure CAPTURES it (the lambda's proto shows the capture and the
# `IncrefValueRegion` retain right before the `TailCall`). Only a
# compile-time-CONSTANT heap value (a stdlib-export closure — `immutable_values`)
# skips the capture and reads as `LoadConst`; that route is the CONST borrow arm,
# pinned by region-const-tail-move-borrow-uaf.lisp. docs/impl/region/rules.md
# Rule 5 (the borrowed tail-call argument escape site).

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
# The counter-factual: the region is over-freed mid-loop (crash) or this read faults.
# Correct: returns "z".
(assert (= (first s) "z")
        "a top-level binding tail-moved into an owned-param callee was over-released")

(println "region-tail-move-toplevel-uaf: ok")
