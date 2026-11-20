(elle/epoch 18)
# audited: 2026-11-20
# Binds each submodule of std/check, made and passed on as check.lisp makes and passes them.
# docs/stdlib/check/overview.md
# tests/lang/overview.md

(def bits ((import "std/check/bits")))
(def errors ((import "std/check/errors")))
(def source ((import "std/check/source") :bits bits :errors errors))
(def random ((import "std/check/random") :bits bits :source source))
(def walk ((import "std/check/walk") :bits bits :source source))
(def gen ((import "std/check/gen") :errors errors))
(def case ((import "std/check/case") :errors errors :gen gen))
(def shrink ((import "std/check/shrink") :bits bits :source source :case case))
(def run
  ((import "std/check/run") :bits bits :errors errors :source source
                            :random random :walk walk :case case :shrink shrink))
