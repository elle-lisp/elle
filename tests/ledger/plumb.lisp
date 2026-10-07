(elle/epoch 13)
# audited: 2026-09-30
# The ledger of tests/impl/plumb.lisp, the I/O leak dashboard: every probe on the object and the region count, pinned at 0.
(producer "tests/impl/plumb.lisp")
["objects gauge (live-growth)" :objects :floor 0.5 :class :growth]
["regions gauge (live-growth)" :regions :floor 0.5 :class :growth]
["io-yield ev/sleep" :objects 0]
["io-yield ev/sleep" :regions 0]
["io-drop" :objects 0]
["io-drop" :regions 0]
["io-abort" :objects 0]
["io-abort" :regions 0]
["io-refuse" :objects 0]
["io-refuse" :regions 0]
["ev-abort" :objects 0]
["ev-abort" :regions 0]
["ev-timeout" :objects 0]
["ev-timeout" :regions 0]
["subprocess-exec" :objects 0 :note "a block here is a block of child processes"]
["subprocess-exec" :regions 0 :note "a block here is a block of child processes"]
["port-read-all" :objects 0]
["port-read-all" :regions 0]
["subprocess-system" :objects 0 :note
 "a block here is a block of child processes"]
["subprocess-system" :regions 0 :note
 "a block here is a block of child processes"]
# The double-release counter, read once after the last probe
# (docs/impl/region/diagnostics.md).
["over-free" :releases 0]
