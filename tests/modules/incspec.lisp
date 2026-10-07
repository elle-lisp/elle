(elle/epoch 14)
# audited: 2026-10-06
# A macro library tests/lang/include.lisp includes through import/resolve.
# docs/modules.md

(defmacro quadruple-it (x)
  `(* ,x 4))
