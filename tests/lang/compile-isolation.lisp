(elle/epoch 15)
# audited: 2026-10-13
# A file's definitions reach no other compile: an earlier load never changes what a later file computes.
# docs/modules.md

# Each case loads a file that defines a function under a global name, then a
# file that calls the global. The counter-factual recorded every file's
# functions under their names for later compiles to splice in, so the second
# file ran the first file's body.

# A function a `map` over a literal array calls. The counter-factual grafted
# the defining file's body into the fused loop and answered [999 999].
(with-temp-dir dir
               (let [define (path/join dir "define.lisp")
                     use (path/join dir "use.lisp")]
                 (file/write define
                             "(defn length [x] 999)\n(fn [] {:length length})\n")
                 (file/write use "(map length [[1 2] [3]])\n")
                 (assert (= [2 1] (import-file use))
                         "use alone runs the global length")
                 (import-file define)
                 (assert (= [2 1] (import-file use))
                         "an earlier file's length does not reach a later map")))

# A function that dispatches on its argument's type. The counter-factual
# collapsed the later call to the defining file's :array arm, and answered
# "[1 2]".
(with-temp-dir dir
               (let [define (path/join dir "define.lisp")
                     use (path/join dir "use.lisp")]
                 (file/write define
                             "(defn length [c] (match (type-of c) :array (string c) _ 0))\n(fn [] {:length length})\n")
                 (file/write use "(length [1 2])\n")
                 (assert (= 2 (import-file use))
                         "use alone runs the global length")
                 (import-file define)
                 (assert (= 2 (import-file use))
                         "an earlier file's length does not reach a later call")))

(println "compile-isolation: OK")
