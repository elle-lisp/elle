(elle/epoch 15)
# audited: 2026-11-18
# region-jit-closure.lisp with guardfree armed.
# docs/impl/jit-closure.md
#
# A released page is unmapped, so a closure, a cell or a merged arena freed
# while something still reads it faults at that read.

(include-file "region-jit-closure.lisp")
