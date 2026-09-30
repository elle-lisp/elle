(elle/epoch 13)
# audited: 2026-09-30
# The ledger of tests/impl/region-page-recycle.lisp: the pages each call shape claims per call, and the bytes a recycling loop keeps.
(producer "tests/impl/region-page-recycle.lisp")
["pages gauge (live-growth)" :pages :floor 0.5 :class :growth]
["bytes gauge (live-growth)" :bytes :floor 1000 :class :growth]
# The free shapes: an intrinsic, a fixed-arity call, and a variadic call whose
# rest list is empty. A pin at 0 admits nothing.
["empty thunk" :pages 0]
["%add" :pages 0]
["fixed-arity call" :pages 0]
["(< a b)" :pages 0 :note "the rest list is empty at two arguments"]
# The rest-list shapes: every cons is born in its own region, which owns a
# page, so the count is the number of rest arguments.
["(vnop 1)" :pages 1]
["(vnop 1 2)" :pages 2]
# The arithmetic wrappers: the rest conses plus the body's letrec closure.
["(+ a b)" :pages 3]
["(+ a b c)" :pages 4]
["(* a b)" :pages 3]
["(- a b)" :pages 2 :note "the first operand is a fixed parameter"]
# The pages a loop claims are recycled, not accumulated: the region dies at
# the end of its call and the next call claims the same page, so the heap's
# byte footprint holds flat while the claim count climbs.
["(+ a b)" :bytes 0]
