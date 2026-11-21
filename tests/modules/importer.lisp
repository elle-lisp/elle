(elle/epoch 18)
# audited: 2026-11-20
# A module whose function imports a module beside it, for tests/lang/import-request.lisp.
# docs/modules/resolution.md

(fn [] {:load (fn [] ((import "./test")))})
