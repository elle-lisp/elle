(elle/epoch 15)
# audited: 2026-10-14
# region-jit-halt-emit.lisp with guardfree armed, so a freed halt payload faults at its read.
# docs/impl/region/park.md
#
# The sidecar turns the graft off for the reason the included file gives.

(include-file "region-jit-halt-emit.lisp")
