(elle/epoch 14)
# audited: 2026-10-06
# One half of a top-level import cycle, for tests/lang/import-loaders.lisp.
# docs/modules.md

(def b ((import "./cycle-b")))
(fn [] {:b b})
