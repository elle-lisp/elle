(elle/epoch 15)
# audited: 2026-11-17
# jit-self-tail-bind.lisp with guardfree armed.
# docs/impl/jit.md
#
# A released page is unmapped, so a loop that releases a moved argument the
# rest list or a fixed parameter still holds faults at the next read of it.

(include-file "jit-self-tail-bind.lisp")
