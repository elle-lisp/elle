(elle/epoch 15)
# audited: 2026-11-11
# region-jit-tail-dispatch-args.lisp with guardfree armed.
# docs/impl/region/mechanism.md
#
# A released page is unmapped, so an argument freed while the compiled callee
# or its caller still reads it faults at that read.

(include-file "region-jit-tail-dispatch-args.lisp")
