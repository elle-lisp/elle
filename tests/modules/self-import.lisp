(elle/epoch 12)
# audited: 2026-09-28
# tests/modules/self-import.lisp — a module that imports itself: the inner
# import is the cycle the loader refuses.
(import-file "tests/modules/self-import.lisp")
