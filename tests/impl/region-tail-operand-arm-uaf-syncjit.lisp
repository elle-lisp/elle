(elle/epoch 15)
# audited: 2026-11-13
# region-tail-operand-arm-uaf.lisp with every function compiled on its first call, on the VM thread.
# docs/impl/region/operand.md
#
# The JIT runs a self tail call as a loop over one activation. An arm that
# released a slot only its sibling writes would free the pair the previous trip
# moved to the call, wherever a trip starts with the slot still holding it. The
# background worker would leave `keep-if` interpreted for its first trips, where
# each frame starts the slot `nil`.

(include-file "region-tail-operand-arm-uaf.lisp")
