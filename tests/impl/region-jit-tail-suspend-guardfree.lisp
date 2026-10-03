(elle/epoch 13)
# audited: 2026-09-29
# region-jit-tail-suspend.lisp compiled on each function's first call, with guardfree armed.
# docs/impl/region/park.md
#
# A released page is unmapped, so a replayed release that frees an argument its
# caller still reads faults at that read.

(include-file "region-jit-tail-suspend.lisp")
