(elle/epoch 12)
# tests/elle/string-push-value.lisp — %string-push must accept an @string
# VALUE on the JIT tier, agreeing with the VM (bytecode) tier.
#
# Counterfactual for the JIT/VM parity bug: `prim_string_push`
# (src/primitives/intrinsics.rs) reads a pushed @string value's bytes via
# `as_string_mut()`, but the JIT runtime intrinsic `elle_jit_string_push`
# (src/jit/runtime.rs) only handled `with_string` and PANICKED on an
# @string value ("%string-push: value must be string, got @string").
# `push-all`/`concat` feed the source collection directly, so an @string
# source reaches %string-push as the pushed value — fine in the VM, an
# abort once the function was JIT-compiled (adaptive/default mode only,
# which the runner's policy passes never exercise; this file forces the
# tier deterministically via `compile/run-on :jit`).
#
# The policy passes never ran the value-is-@string case through the JIT,
# so it hid. This test pins JIT==VM agreement for it.

# Gate the whole file on JIT availability: it exists to pin JIT==VM parity, so a
# build with no JIT tier compiled in (--no-default-features, e.g. the aarch64
# no-features job) has nothing to compare — (compile/run-on :jit …) then returns
# :tier-rejected. Re-raise as a loud :gated so `elle test` records a file-level
# SKIP with a reason and a direct run prints "SKIP (gated)" (exit 0), matching the
# lib-load gates in compress.lisp / git.lisp. Eager (def …) so it gates during
# barrier-module setup, before any test thunk runs.
(def _jit-available
  (let [[ok? v] (protect (compile/run-on :jit (fn [] 0)))]
    (if (and (not ok?) (= (get v :error) :tier-rejected))
      (error (struct :error :gated :reason "JIT tier not compiled in"))
      true)))

# A closure that pushes its second arg onto its first and returns the first.
# The `(match (type-of …))` arm proves `dst` authoritatively for the raw
# intrinsic (docs/intrinsics.md § What counts as proof) — the pin is
# %string-push itself, never a wrapper.
(def push-onto
  (fn [dst v]
    (match (type-of dst)
      :@string (begin
                 (%string-push dst v)
                 dst)
      _ (error {:error :type-error :message "push-onto: @string dst required"}))))

# Case 1: pushed value is an @string (the failing case).
(defn case-mut-value [tier]
  (let [@src (@string)]
    (%string-push src "abc")
    (freeze (compile/run-on tier push-onto (@string) src))))

(assert (= (case-mut-value :bytecode) "abc")
        "VM: %string-push accepts an @string value")
(assert (= (case-mut-value :jit) "abc")
        "JIT: %string-push accepts an @string value")
(assert (= (case-mut-value :bytecode) (case-mut-value :jit))
        "JIT and VM agree on pushing an @string value")

# Case 2: pushed value is an immutable string (must still work post-fix).
(defn case-imm-value [tier]
  (freeze (compile/run-on tier push-onto (@string) "xyz")))

(assert (= (case-imm-value :jit) "xyz")
        "JIT: %string-push still accepts an immutable string value")
(assert (= (case-imm-value :bytecode) (case-imm-value :jit))
        "JIT and VM agree on pushing an immutable string value")

# Case 3: both collection AND value are @string (aliasing-safe read: the
# value's bytes are copied out before the collection is mutably borrowed).
(defn case-mut-both [tier]
  (let [@src (@string)]
    (%string-push src "de")
    (let [@dst (@string)]
      (%string-push dst "abc")
      (freeze (compile/run-on tier push-onto dst src)))))

(assert (= (case-mut-both :jit) "abcde")
        "JIT: @string collection + @string value bulk-appends")
(assert (= (case-mut-both :bytecode) (case-mut-both :jit))
        "JIT and VM agree on @string + @string")

(println "string-push-value: OK")
