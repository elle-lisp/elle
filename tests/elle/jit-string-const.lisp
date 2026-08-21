(elle/epoch 12)
## jit/string-const — a JIT-forced closure with a String constant must compile.
##
## RED counterfactual for the forced-tier JIT path. The adaptive JIT path
## (jit_entry::submit_jit_task) pre-resolves every `LirConst::String` to a
## `ValueConst` via `jit::worker::prepare_task` BEFORE the LIR reaches the JIT
## translator — the translator has no handler for a raw `LirConst::String` and
## hits `unreachable!` (src/jit/helpers.rs). The forced-tier path
## (`compile/run-on :jit` → invoke_closure_jit) compiled the raw LIR directly
## and skipped that pre-resolution, so any forced-JIT closure carrying a string
## literal aborted the process — non-unwinding, so it bypassed the test runner's
## fault barrier and killed the whole run. `compile/run-on :jit` force-compiles
## regardless of the active JIT policy, so this exercises the path on every tier.

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

## A string constant flowing through a larger expression (still a Const in LIR).
(assert (= "ab" (compile/run-on :jit (fn [] (concat "a" "b"))))
        "forced-JIT closure with string constants in a call")

## A string constant chosen by control flow — both arms carry Const strings.
(assert (= "yes" (compile/run-on :jit (fn [x] (if x "yes" "no")) true))
        "forced-JIT closure returns a branch's string constant")

(println "jit-string-const: ok")
