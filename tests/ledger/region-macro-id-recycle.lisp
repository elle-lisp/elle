(elle/epoch 13)
# audited: 2026-09-30
# The ledger of tests/impl/region-macro-id-recycle.lisp: the physical region ids a macro expansion strands per call.
(producer "tests/impl/region-macro-id-recycle.lisp")
["ids gauge (live-growth)" :ids :floor 0.5 :class :growth]
["objects gauge (live-growth)" :objects :floor 0.5 :class :growth]
["regions gauge (live-growth)" :regions :floor 0.5 :class :growth]
["(when true 1)" :ids 0]
["(unless false 1)" :ids 0]
["(case 1 1 2)" :ids 0]
["(when true (+ 1 2))" :ids 0]
["(if true 1 2)" :ids 0]
["(when true 1)" :objects 0]
["(when true 1)" :regions 0]
