(elle/epoch 14)
# audited: 2026-10-06
# A macro whose expansion asks for its own location, for tests/lang/meta-location.lisp.
# docs/modules.md

(defmacro where []
  `(meta/location))
