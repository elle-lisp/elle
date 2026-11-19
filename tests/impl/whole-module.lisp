(elle/epoch 18)
# audited: 2026-11-19
# A whole-module thunk runs a file's forms under the file's own top-level rules, as a direct run does.
# docs/test-runner.md
#
# compile/whole-module wraps a file's forms in one deferred thunk and runs it
# once per tier. The thunk wraps its body in the internal `%file-body` form, so
# the analyzer runs the same analyze_file_letrec a real file gets
# (docs/bindings.md).

(defn whole-run [src]
  "Compile SRC as one whole-file thunk and run it on the bytecode tier,
   returning the thunk's value — exactly what the runner does per tier."
  (let [thunk (get (get (compile/whole-module src "<wm-test>") 0) 1)]
    (compile/run-on :bytecode thunk)))

(defn whole-refusal [src]
  "The message compile/whole-module reports for SRC, which must not compile."
  (let [[ok? err] (protect (compile/whole-module src "<wm-test>"))]
    (assert (not ok?) "the program must not compile")
    (get err :message)))

# 1. A def whose value reads its own name reads it before its initialization,
#    in the thunk as in a file.
(assert (string/contains? (whole-refusal "(def a 10)\n(def a (+ a 1))\na")
                          "'a' referenced before its initialization")
        "whole-module: (def a 10)(def a (+ a 1)) is refused as in a file")

# 2. The same holds for a mutable redefinition.
(assert (string/contains? (whole-refusal
                            "(def @x @[1 2 3])\n(def @x (freeze x))\nx")
                          "'x' referenced before its initialization")
        "whole-module: (def @x (freeze x)) is refused as in a file")

# 3. A name exists from its def onward: a call to a later form's function does
#    not compile, and functions that call each other bind in one letrec.
(assert (string/contains? (whole-refusal
                            (concat "(defn evn? [n] (if (= n 0) true (od? (- n 1))))\n"
                                    "(defn od? [n] (if (= n 0) false (evn? (- n 1))))\n"
                                    "(evn? 10)")) "undefined variable: od?")
        "whole-module: a call to a later form's function is refused")
(assert (= (whole-run (concat "(letrec [evn? (fn [n] (if (= n 0) true (od? (- n 1))))\n"
                              "         od? (fn [n] (if (= n 0) false (evn? (- n 1))))]\n"
                              "  (evn? 10))")) true)
        "whole-module: mutual recursion in a letrec resolves")

# 4. the thunk's value is the last form's value — matches a direct run.
(assert (= (whole-run "(def a 1)\n(def b 2)\n(+ a b)") 3)
        "whole-module: thunk returns the last form's value")

(println "whole-module tests passed")
