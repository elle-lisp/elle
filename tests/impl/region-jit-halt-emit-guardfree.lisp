(elle/epoch 15)
# audited: 2026-10-06
# region-jit-halt-emit.lisp with guardfree armed, so a freed halt payload faults at its read.
# docs/impl/region/park.md

(include-file "region-jit-halt-emit.lisp")
