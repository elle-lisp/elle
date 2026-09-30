(elle/epoch 13)
# audited: 2026-09-30
# The ledger of tests/impl/h2-stress-scoped.lisp: what a scoped h2 request loop leaves behind per request, at two counts.
(producer "tests/impl/h2-stress-scoped.lisp")
["objects gauge (live-growth)" :objects :floor 0.5 :class :growth]
["regions gauge (live-growth)" :regions :floor 0.5 :class :growth]
# The sequential loop at two request counts. A residue that grows faster than
# the request count reads differently at the two, and a one-off the lead run
# did not absorb reads as a fraction that shrinks with the count.
["sequential, 10 requests" :objects 0]
["sequential, 10 requests" :regions 0]
["sequential, 30 requests" :objects 0]
["sequential, 30 requests" :regions 0]
