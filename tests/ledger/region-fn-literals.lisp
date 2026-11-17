(elle/epoch 15)
# audited: 2026-11-16
# The ledger of tests/impl/region-fn-literals.lisp: the pages a walk over a literal predicate claims, and the time it takes against a walk over a global.
(producer "tests/impl/region-fn-literals.lisp")
["pages gauge (live-growth)" :pages :floor 0.5 :class :growth]
# The unit builds each literal once, when it starts, so a call claims
# nothing: the literal predicates read what the global ones read.
["a string literal, per walk of 1000 calls" :pages 0]
["a global string, per walk of 1000 calls" :pages 0]
["a set literal, per walk of 1000 calls" :pages 0]
["a global set, per walk of 1000 calls" :pages 0]
# A literal is a value the predicate captures, as the global is, so the
# ratio is 1 and the machine decides the rest.
["a string literal against a global" :time 1 :slack 0.5]
["a set literal against a global" :time 1 :slack 0.5]
