(elle/epoch 14)
# audited: 2026-10-06
# tests/modules/self-import.lisp — a module that imports itself: the inner
# import is the cycle the loader refuses.
(import-file "self-import.lisp")
