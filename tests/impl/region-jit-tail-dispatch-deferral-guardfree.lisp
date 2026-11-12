(elle/epoch 15)
# audited: 2026-11-11
# region-jit-tail-dispatch-deferral.lisp with guardfree armed.
# docs/impl/region/relocate.md
#
# A released page is unmapped, so an adder freed while it still reads its
# captured value faults at that read.

(include-file "region-jit-tail-dispatch-deferral.lisp")
