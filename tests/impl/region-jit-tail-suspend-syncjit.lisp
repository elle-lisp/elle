(elle/epoch 13)
# audited: 2026-09-29
# region-jit-tail-suspend.lisp with every function compiled on its first call, on the VM thread.
# docs/impl/region/park.md
#
# Each face then runs compiled from its first call, so the gauge reads compiled
# tail calls alone.

(include-file "region-jit-tail-suspend.lisp")
