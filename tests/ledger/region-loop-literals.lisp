(elle/epoch 15)
# audited: 2026-11-11
# The ledger of tests/impl/region-loop-literals.lisp: the pages a scan over a literal claims per entry, and the time it takes against a scan over a global.
(producer "tests/impl/region-loop-literals.lisp")
["pages gauge (live-growth)" :pages :floor 0.5 :class :growth]
# One page per value the literal builds, once per entry: the set and the
# struct each hold two strings, and each of the three is a region.
["a string literal, per scan of 1000 passes" :pages 1]
["a global string, per scan of 1000 passes" :pages 0]
["a set literal, per scan of 1000 passes" :pages 3]
["a global set, per scan of 1000 passes" :pages 0]
["a struct literal, per scan of 1000 passes" :pages 3]
["a global struct, per scan of 1000 passes" :pages 0]
# A hoisted literal is a binding the loop reads, as the global is, so the
# ratio is 1 and the machine decides the rest.
["a string literal against a global" :time 1 :slack 0.5]
["a set literal against a global" :time 1 :slack 0.5]
