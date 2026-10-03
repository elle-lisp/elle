(elle/epoch 13)
# audited: 2026-09-29
# A portrait's composition reports whether this implementation's JIT admits the function.
# docs/analysis/portrait.md
#
# `:jit-eligible` describes this implementation's JIT, so it is a fact about
# this implementation rather than about the language. tests/lang/portrait.lisp
# holds the rest of the library's contract.

(def portrait ((import "std/portrait")))

# add calls stdlib +, which may error on a non-numeric argument, and a
# function that may error is not eligible.
(def p (portrait:function (compile/analyze "(defn add [a b] (+ a b))") :add))
(assert (not (get (get p :composition) :jit-eligible))
        "add not jit-eligible (may error)")

(println "portrait-jit-eligible: ok")
