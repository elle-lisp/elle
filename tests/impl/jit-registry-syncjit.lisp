(elle/epoch 18)
# audited: 2026-11-26
# jit-registry.lisp with every function compiled on its first call, on the VM thread.
# docs/impl/jit-map.md
#
# The VM thread compiles a task without the worker, so this run checks the
# label that compile records.

(include-file "jit-registry.lisp")
