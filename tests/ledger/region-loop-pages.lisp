(elle/epoch 15)
# audited: 2026-10-20
# The ledger of tests/impl/region-loop-pages.lisp: the pages a loop that allocates nothing claims per iteration, and the time its wrapper spelling takes against its intrinsic one.
(producer "tests/impl/region-loop-pages.lisp")
["pages gauge (live-growth)" :pages :floor 0.5 :class :growth]
["counting over %-ops" :pages 0]
["counting over wrappers" :pages 0]
["counting in a function" :pages 0]
["counting over an unproven bound" :pages 0]
["counting over a captured cell" :pages 0]
["a CRC-32 step per iteration" :pages 0]
["a struct read per iteration" :pages 0]
# The wrapper loop pays the let the graft writes for a written operand (#1405),
# and the machine decides the rest; the graft off reads about 18.
["counting over wrappers against %-ops" :time 1.9 :slack 0.5]
