(elle/epoch 13)
# audited: 2026-09-29
# jit-compiled-caller-promotes-callee.lisp with every function compiled on its
# first call, on the VM thread.
# docs/impl/jit.md
#
# The caller is compiled before it makes its first call, so it has no
# interpreted window at all, and the callee is reached only through
# `elle_jit_call` from the start.

(include-file "jit-compiled-caller-promotes-callee.lisp")
