(elle/epoch 12)
# audited: 2026-09-20
# hello.lisp — what the Rust embedding host runs: a call into a host-provided
# primitive, and an answer the host has to give a reference back for.
#
# docs/embedding.md

(println "Elle says hello!")
(println "host/add-ten of 32 =" (host/add-ten 32))

# The answer is a heap value, which is the case the host's hand-off exists for:
# an immediate occupies no region, so a host that drops one owes nothing.
(list 1 2 3)
