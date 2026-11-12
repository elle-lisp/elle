(elle/epoch 15)
# audited: 2026-11-11
# jit-tail-dispatch-yield.lisp with guardfree armed.
# docs/impl/region/owner.md
#
# A released page is unmapped, so a release that frees an argument or a frame's
# value while something still reads it faults at that read.

(include-file "jit-tail-dispatch-yield.lisp")
