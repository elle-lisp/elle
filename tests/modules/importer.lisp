(elle/epoch 14)
# audited: 2026-10-06
# A module whose function imports a module beside it, for tests/lang/modules.lisp.
# docs/modules.md

(fn [] {:load (fn [] ((import "./test")))})
