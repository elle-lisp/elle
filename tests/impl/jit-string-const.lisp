(elle/epoch 14)
# audited: 2026-10-06
# A closure forced onto the JIT compiles and runs with a string literal in its body.
# docs/impl/jit.md
#
# A string literal lowers to `MaterializeConst`, which the JIT translates by
# calling `elle_jit_materialize_const`. `compile/run-on :jit` compiles the
# closure's LIR directly, whatever the active JIT policy, so this file reaches
# the translator on every tier. The trap: a translator that cannot place a
# string aborts the process, which no fault barrier catches, so the whole run
# dies rather than this file failing.

# Gate on JIT availability: a build with no JIT tier compiled in
# (--no-default-features, e.g. the aarch64 no-features job) rejects
# (compile/run-on :jit …) with :error :tier-rejected. This file exercises the
# forced :jit tier, so re-raise as a loud :gated — `elle test` records a file-level
# SKIP and a direct run prints "SKIP (gated)" (exit 0), matching compress.lisp.
(def _jit-available
  (let [[ok? v] (protect (compile/run-on :jit (fn [] 0)))]
    (if (and (not ok?) (= (get v :error) :tier-rejected))
      (error (struct :error :gated :reason "JIT tier not compiled in"))
      true)))

## A closure whose body is a bare string constant.
(assert (= "hello" (compile/run-on :jit (fn [] "hello")))
        "forced-JIT closure returns its string constant")

## A string constant flowing through a larger expression.
(assert (= "ab" (compile/run-on :jit (fn [] (concat "a" "b"))))
        "forced-JIT closure with string constants in a call")

## A string constant chosen by control flow, one in each arm.
(assert (= "yes" (compile/run-on :jit (fn [x] (if x "yes" "no")) true))
        "forced-JIT closure returns a branch's string constant")

(println "jit-string-const: ok")
