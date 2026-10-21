(elle/epoch 15)
# audited: 2026-10-21
# A macro library that tests/lang/include.lisp and docs/include.md include.
# docs/include.md

(defmacro quadruple-it (x)
  `(* ,x 4))
