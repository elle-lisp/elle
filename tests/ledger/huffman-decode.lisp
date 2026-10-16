(elle/epoch 15)
# audited: 2026-10-14
# The ledger of tests/impl/huffman-decode.lisp: the pages one Huffman decode claims, per value.
(producer "tests/impl/huffman-decode.lisp")
["pages gauge (live-growth)" :pages :floor 0.5 :class :growth]
["decode 100 short codes" :pages 3]
["decode 100 long codes" :pages 3]
