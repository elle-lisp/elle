(elle/epoch 15)
# audited: 2026-10-21
# region-error-park-uaf.lisp with every function compiled on its first call, on the VM thread.
#
# docs/impl/jit.md
#
# Compiled code runs a function from its second call on, so the drive's first
# iteration warms every callee and the rest raise from compiled code, which
# parks the frame the same way. The asserts after the include confirm the
# callees ran compiled. The sidecar keeps guardfree armed, and turns the graft
# off. Under the graft, a call to a function the compiler can see becomes the
# function's body (docs/impl/graft.md), and a callee nothing calls never
# compiles.

(include-file "region-error-park-uaf.lisp")

(assert (and (jit? holder) (jit? outer-holder))
        "the restarted chain is compiled")
(assert (and (jit? payload-holder) (jit? payload-outer))
        "the discarded chain is compiled")
