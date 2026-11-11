(elle/epoch 15)
# audited: 2026-11-11
# The ledger of tests/impl/region-loop-literals.lisp: the pages a loop over a literal claims per iteration, and the time it takes against a loop over a global.
(producer "tests/impl/region-loop-literals.lisp")
["pages gauge (live-growth)" :pages :floor 0.5 :class :growth]
["a string literal per iteration" :pages 0]
["a global string per iteration" :pages 0]
["a set literal per iteration" :pages 0]
["a global set per iteration" :pages 0]
["a struct literal per iteration" :pages 0]
["a global struct per iteration" :pages 0]
# A hoisted literal is a binding the loop reads, as the global is, so the
# ratio is 1 and the machine decides the rest.
["a string literal against a global" :time 1 :slack 0.5]
["a set literal against a global" :time 1 :slack 0.5]
