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

# A closure that pushes its second arg onto its first and returns the first.
(def push-onto
  (fn [dst v]
    (%string-push dst v)
    dst))

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
