(elle/epoch 18)
# audited: 2026-11-20
# A macro whose expansion asks for its own location, for tests/lang/meta-location.lisp.
# docs/modules/resolution.md

(defmacro where []
  `(meta/location))
