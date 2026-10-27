(elle/epoch 15)
# audited: 2026-10-26
# A module whose function imports a module beside it, for tests/lang/import-request.lisp.
# docs/modules.md

(fn [] {:load (fn [] ((import "./test")))})
