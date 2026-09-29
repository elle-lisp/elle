(elle/epoch 12)
# Regression: the legacy multi-form path (compile/whole-module) wraps a file's
# forms into ONE deferred thunk and runs it once per tier. That thunk MUST
# reproduce the file's TOP-LEVEL (file-scope letrec) semantics — not fn-body
# internal-define semantics — or an imperative script diverges from a direct
# `elle FILE` run. The sharp case is def REDEFINITION: at file scope a
# redefinition's RHS sees the PREVIOUS binding ((def a 10) (def a (+ a 1)) → 11),
# whereas a naive (fn () forms…) wrapper hoists both into one internal-define
# frame (→ 10). The whole-module thunk wraps its body in the internal `%file-body`
# form so the analyzer runs the same analyze_file_letrec a real file gets.

(defn whole-run [src]
  "Compile SRC as one whole-file thunk and run it on the bytecode tier,
   returning the thunk's value — exactly what the runner does per tier."
  (let [thunk (get (get (compile/whole-module src "<wm-test>") 0) 1)]
    (compile/run-on :bytecode thunk)))

# 1. def REDEFINITION: the redefinition's RHS sees the previous binding.
(assert (= (whole-run "(def a 10)\n(def a (+ a 1))\na") 11)
        "whole-module: (def a 10)(def a (+ a 1)) must yield 11 (file-scope redef)")

# 2. @x then x (stripped-name shadow): the second def's RHS sees the previous @x,
#    not a fresh uninitialized binding.
(assert (= (whole-run "(def @x @[1 2 3])\n(def x (freeze x))\nx") [1 2 3])
        "whole-module: freeze of @x must produce [1 2 3] (file-scope shadow)")

# 3. forward reference / mutual recursion across forms still resolves (the
#    prebind pass that file-scope letrec performs must be preserved).
(assert (= (whole-run (concat "(defn evn? [n] (if (= n 0) true (od? (- n 1))))\n"
                              "(defn od? [n] (if (= n 0) false (evn? (- n 1))))\n"
                              "(evn? 10)")) true)
        "whole-module: mutual recursion across forms must resolve")

# 4. the thunk's value is the last form's value — matches a direct run.
(assert (= (whole-run "(def a 1)\n(def b 2)\n(+ a b)") 3)
        "whole-module: thunk returns the last form's value")

(println "whole-module tests passed")
