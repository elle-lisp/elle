(elle/epoch 15)
# audited: 2026-11-12
# The ledger of tests/impl/write-copies.lisp: the page bytes a write claims per byte it sends, beside a concat that must claim one.
(producer "tests/impl/write-copies.lisp")
["concat then port/write" :copies :floor 0.75 :class :growth]
["port/write" :copies :ceiling 0.25]
["port/write-many" :copies :ceiling 0.25]
["gather flush" :copies :ceiling 0.25]
["http transport" :copies :ceiling 0.25]
["http2 frame" :copies :ceiling 0.25]
["websocket send" :copies :ceiling 0.25]
["websocket text" :copies :ceiling 0.25]
["redis command" :copies :ceiling 0.25]
["redis pipeline" :copies :ceiling 0.25]
