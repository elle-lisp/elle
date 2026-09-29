(elle/epoch 13)
# audited: 2026-09-29
# A value a compiled function emits keeps its escape retain, so the resumer's fiber/value read finds it live.
# docs/impl/jit.md
#
# The interpreter's `Emit` handler (`handle_emit`, src/vm/dispatch.rs) increfs
# the emitted value's region (`EscapeSite::EmitEscape`) before it stores it into
# `fiber.signal`, so the compiler's `DecrefRegion` at the emit's decref_point does
# not drop the value's only reference while the resumer still holds it. The
# JIT's `Emit` side-exit (`elle_jit_yield`, src/jit/suspend.rs) takes the same
# retain. This is the emit-terminator twin of
# tests/impl/region-jit-io-suspend-uaf.lisp, the retain for a yielding NATIVE.
#
# The counter-factual: a side-exit without the retain frees the emitted struct at
# the decref_point, so `(fiber/value gen)` derefs a stale region — the debug
# generation guard panics, and a release build reads garbage fields. The
# interpreter always retains, so the file passes with the JIT off too.
#
# REACHABILITY (why a hot per-call emitter, not one big loop): the JIT compiles
# a function only once it is HOT (call-counted). `step` does exactly ONE emit of
# a freshly-built struct per call, and is called per iteration, so thousands of
# calls drive it hot; once compiled, every emit takes the JIT side-exit. A
# function that loops the emits internally is called once, never gets hot, and
# stays interpreted and never takes the side-exit.

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
