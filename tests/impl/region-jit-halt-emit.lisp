(elle/epoch 15)
# audited: 2026-10-14
# A compiled emit of :halt takes no delivery retain, so a halted fiber strands nothing.
# docs/impl/region/park.md
#
# The sidecar turns the graft off: a call to a function the compiler can see
# becomes the function's body (docs/impl/graft.md), and `body` nothing calls
# never compiles.
#
# A halted fiber is promoted to :dead and never resumed, so the delivery a
# suspending emit retains for has no consumer. The interpreter's `Emit` handler
# therefore takes neither the retain nor the ledger record for a halt. The JIT's
# side-exit must skip both as well. One that takes them strands the payload's
# region once per halted fiber, so the window below reads about `window` where it
# should read near 0. The interpreter never takes them, so the gauge reads 0
# there, and only a compiled `body` tells the two apart.
#
# The trap: `body` compiles only if a counted call reaches it. A fiber's first
# resume enters its thunk without one, and a tail call is counted by neither
# tier. So the thunk calls `body` in a non-tail position, and the assertion on
# `(jit? body)` keeps the gauge from passing on interpreted code.
#
# The payload's read after each halt is the soundness face: the park retain of
# the terminal result pins it, not the delivery retain the side-exit gives up.
# region-jit-halt-emit-guardfree.lisp runs this file with guardfree armed, which
# faults on a freed payload.
#
# In a build without the JIT `(vm/config :jit)` reads nil and nothing compiles,
# so the gauge needs a JIT.

(defn body []
  (emit :halt @[1 2]))

(defn thunk []
  (body)
  :unreached)

(defn once []
  (let [f (fiber/new thunk |:halt|)]
    (fiber/resume f)
    (assert (= :dead (fiber/status f)) "a halted fiber is dead")
    (assert (= 2 (get (fiber/value f) 1)) "the halted payload was freed")))

(def window 400)

(defn measure [face]
  (var i 0)
  (while (%lt i 30)
    (face)
    (assign i (%add i 1)))
  # Compiles run on a worker; wait for them, so the window runs compiled code.
  (jit/rejections)
  (def before (arena/count))
  (var j 0)
  (while (%lt j window)
    (face)
    (assign j (%add j 1)))
  (%sub (arena/count) before))

(when (vm/config :jit)
  (def delta (measure once))
  (assert (jit? body)
          "precondition: `body` must be JIT-compiled, else the gauge reads the interpreter")
  (println "region-jit-halt-emit delta over " window " halts: " delta)
  (assert (%lt delta 100)
          (string "a compiled emit of :halt leaks, delta=" delta)))

(println "region-jit-halt-emit: ok")
