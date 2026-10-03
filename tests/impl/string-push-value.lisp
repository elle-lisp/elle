(elle/epoch 12)
# audited: 2026-09-29
# %string-push accepts an @string value on the JIT tier, and agrees with the interpreter.
# docs/intrinsics.md
#
# `prim_string_push` (src/primitives/intrinsics/data.rs) reads a pushed
# @string value's bytes through `as_string_mut()`, and the JIT's
# `elle_jit_string_push` (src/jit/runtime/ops.rs) must accept the same value.
# `push-all` and `concat` feed the source collection directly, so an @string
# source reaches %string-push as the pushed value.
#
# The counter-factual: a JIT intrinsic that reads only immutable strings
# panics with "%string-push: value must be string, got @string" once the
# function is compiled. `compile/run-on :jit` forces the tier, so the file
# reaches the compiled path on every run rather than after a warm-up.

# A build with no JIT tier answers `(compile/run-on :jit …)` with
# :tier-rejected and has nothing to compare, so the file gates itself there.
# The eager `def` gates before any assertion runs.
(def _jit-available
  (let [[ok? v] (protect (compile/run-on :jit (fn [] 0)))]
    (if (and (not ok?) (= (get v :error) :tier-rejected))
      (error (struct :error :gated :reason "JIT tier not compiled in"))
      true)))

# A closure that pushes its second arg onto its first and returns the first.
# The `(match (type-of …))` arm proves `dst` authoritatively for the raw
# intrinsic (docs/intrinsics.md) — the pin is
# %string-push itself, never a wrapper.
(def push-onto
  (fn [dst v]
    (match (type-of dst)
      :@string (begin
                 (%string-push dst v)
                 dst)
      _ (error {:error :type-error :message "push-onto: @string dst required"}))))

# Case 1: pushed value is an @string.
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

# Case 2: pushed value is an immutable string.
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
