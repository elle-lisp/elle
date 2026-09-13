(elle/epoch 12)
# audited: 2026-09-11
# JIT promotion across the tier boundary (docs/impl/jit.md, "Function selection").
#
# A non-tail call is counted by whichever tier makes it. `hand-to` is warmed on
# `decoy` until it compiles, so every later `(hand-to callee 1)` runs as native
# code and reaches `callee` through `elle_jit_call`. Count only the
# interpreter's calls and `callee` is called from nowhere the counter can see:
# it never becomes hot, never compiles, and `(jit? callee)` reads false.
#
# The trap: the background worker hides this. A caller keeps running
# interpreted while Cranelift works, which is long enough for its callees to be
# counted on the interpreter's path, so the reading depends on how fast the
# worker happens to be. `(jit/rejections)` drains pending compilations as a
# side effect, so the first drain puts `hand-to` in the cache before `callee`
# is called at all, and the answer stops depending on timing.
# `--trace=syncjit` reaches the same state by installing on the first call.
#
# The counter-factual: `(f x)` sits under a `begin` on purpose. In tail
# position it lowers to a TailCall, which replaces the frame instead of
# building one and is counted by neither tier — so a tail-position probe
# would read false whatever the call path does, and prove nothing.
#
# Under `--jit=off` nothing compiles and `(jit? f)` is always false, so the
# assertions are gated on an active policy.

(defn hand-to [f x]
  (begin
    (f x)
    0))

(defn decoy [x]
  x)

(defn callee [x]
  (+ x 1))

# Warm the caller alone. `decoy` takes the interpreter's path here, which is
# what leaves `callee` with no interpreted call site of its own.
(def @i 0)
(while (< i 30)
  (hand-to decoy 1)
  (assign i (+ i 1)))
(jit/rejections)

(assign i 0)
(while (< i 30)
  (hand-to callee 1)
  (assign i (+ i 1)))
(jit/rejections)

(when (not (= (vm/config :jit) :off))
  (assert (jit? hand-to)
          "precondition: the caller must be JIT-compiled, else the callee is still reached through the interpreter and the assertion below is vacuous")
  (assert (jit? callee)
          "a callee reached only from compiled code must still be promoted (docs/impl/jit.md, Function selection)"))

(println "ok: a compiled caller promotes its callee")
