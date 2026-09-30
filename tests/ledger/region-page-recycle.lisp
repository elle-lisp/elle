(elle/epoch 13)
# audited: 2026-09-30
# The ledger of tests/impl/region-page-recycle.lisp: the pages each call shape claims per call, and the bytes a recycling loop keeps.
(producer "tests/impl/region-page-recycle.lisp")
["pages gauge (live-growth)" :pages :floor 0.5 :class :growth]
["bytes gauge (live-growth)" :bytes :floor 1000 :class :growth]
["empty thunk" :pages 0]
["%add" :pages 0]
["fixed-arity call" :pages 0]
["(< a b)" :pages 0]
["(vnop 1)" :pages 1]
["(vnop 1 2)" :pages 2]
["(+ a b)" :pages 3]
["(+ a b c)" :pages 4]
["(* a b)" :pages 3]
["(- a b)" :pages 2]
["(+ a b)" :bytes 0]
