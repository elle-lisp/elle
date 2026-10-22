(elle/epoch 15)
# audited: 2026-10-22
# A module whose function imports a module beside it, for tests/lang/import-file.lisp.
# docs/modules.md

(fn [] {:load (fn [] ((import "./test")))})
