(elle/epoch 12)
# Counterfactual: a value EMITTED from JIT-compiled code (the `Emit`/`yield`
# terminator) escapes into `fiber.signal` WITHOUT the escape retain the
# interpreter applies — so the emitted value's region is freed before the
# resumer reads it via `fiber/value`: use-after-free.
#
# THE BUG: the interpreter's `Emit` handler (`handle_emit`, src/vm/dispatch.rs)
# increfs the emitted value's region (`EscapeSite::EmitEscape`) before it stores
# it into `fiber.signal`, so the compiler's `DecrefRegion` at the emit's
# decref_point — which fires as the fiber suspends/continues — does not drop the
# value's only reference while the resumer still holds it via `fiber.signal`.
# The JIT's `Emit` terminator side-exit (`elle_jit_yield`, src/jit/suspend.rs)
# set `fiber.signal` but OMITTED that incref. So when a *JIT-compiled* function
# reaches the emit terminator, the emitted value's region drops to rc 0 at the
# decref_point and is reclaimed; a later `(fiber/value f)` reads a freed struct.
# This is the emit-terminator twin of tests/elle/region-jit-io-suspend-uaf.lisp
# (which is the missing retain for a yielding NATIVE).
#
# REACHABILITY (why a hot per-call emitter, not one big loop): the JIT compiles
# a function only once it is HOT (call-counted). `step` does exactly ONE emit of
# a freshly-built struct per call, and is called per iteration, so thousands of
# calls drive it hot; once compiled, every emit takes the JIT side-exit. A
# function that loops the emits internally is called once, never gets hot, and
# stays interpreted — never exercising the defect.
#
# RED now (JIT tier): the emitted struct's region is freed at the decref_point,
# so `(fiber/value gen)` derefs a stale region — the debug generation guard
# panics (`stale region deref`), or in release the struct's fields read garbage.
# GREEN once `elle_jit_yield` mirrors `handle_emit`'s EmitEscape retain. Under
# `--jit=off` the interpreter already retains, so this file passes on both tiers.

(def iters 40000)

# ONE emit of a fresh heap struct per call → goes hot → JIT-compiled with a
# yield side-exit. The struct is a temporary of the emit expression, so its
# decref_point fires right at the suspend — exactly what the EmitEscape retain
# must survive.
(defn step [i]
  (emit |:yield| {:n i :tag "emit-escape-payload"}))

(def gen
  (fiber/new (fn []
               (var i 0)
               (while (< i iters)
                 (step i)
                 (assign i (+ i 1)))
               :done) |:yield|))

# Driver: resume, then read the emitted struct via fiber/value and verify it is
# intact. A freed region either faults (debug guard) or returns a corrupted
# field (release).
(var i 0)
(var bad 0)
(while (< i iters)
  (fiber/resume gen)
  (let [v (fiber/value gen)]
    (when (or (not (struct? v)) (not (= (get v :n) i))
              (not (= (get v :tag) "emit-escape-payload")))
      (assign bad (+ bad 1))))
  (assign i (+ i 1)))

(assert (= bad 0)
        (string "emitted struct corrupted in " bad " of " iters
                " reads — a JIT emit-escape retain was missing"))
(println "region-jit-emit-escape-uaf: ok")
