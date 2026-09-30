(elle/epoch 13)
# audited: 2026-09-30
# The ledger of tests/impl/region-macro-id-recycle.lisp: the physical region ids a macro expansion strands per call.
(producer "tests/impl/region-macro-id-recycle.lisp")
["ids gauge (live-growth)" :ids :floor 0.5 :class :growth]
["objects gauge (live-growth)" :objects :floor 0.5 :class :growth]
["regions gauge (live-growth)" :regions :floor 0.5 :class :growth]
# Atom arguments: the expansion wraps nothing, so its transient region never
# materializes, and its id must come back by the scope's own close rather than
# by a teardown that can never run.
["(when true 1)" :ids 0]
["(unless false 1)" :ids 0]
["(case 1 1 2)" :ids 0]
# The controls: a compound argument is wrapped, so the region materializes and
# the ordinary teardown returns its id; a special form expands no macro at all.
["(when true (+ 1 2))" :ids 0]
["(if true 1 2)" :ids 0]
# The dimension the other gauges cannot see: the same loop that would strand an
# id per call holds the object and the region count flat.
["(when true 1)" :objects 0]
["(when true 1)" :regions 0]
