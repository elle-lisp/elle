#!/usr/bin/env elle
(elle/epoch 12)

# A halt inside a fiber, from every position that drives a child fiber.
#
# `halt` is maskable but non-resumable: a fiber whose mask names `:halt`
# absorbs the signal and lands `:dead`, holding the halted value
# (docs/signals/primitives.md). The driver releases everything the dead child
# owns at that moment, so the same halt must be recognized identically in call
# position, in tail position, under the JIT, and through the nested-resume
# trampoline. The corpus runs each form on every tier the binary carries, so
# one file covers all four.
#
# The value carried out of the halt is what proves the signal was absorbed
# rather than propagated: a propagating halt would end the run before the next
# assertion.

# ── 1. Call position ───────────────────────────────────────────────
#
# `fiber/resume` in a value position: its result feeds an enclosing form.

(println "  1. halt in call position...")
(let [f (fiber/new (fn [] (halt 42)) |:halt|)
      got (fiber/resume f)]
  (assert (= got 42) "a masked halt delivers its value to the resumer")
  (assert (= (fiber/status f) :dead) "a halted fiber is dead")
  (assert (= (fiber/value f) 42) "a halted fiber holds its value"))
(println "  1. ok")

# ── 2. Tail position ───────────────────────────────────────────────
#
# `fiber/resume` as the last form of a function body, so the driver takes the
# TailCall arm rather than the Call arm.

(println "  2. halt in tail position...")
(defn drive-tail [f]
  (fiber/resume f))
(let [f (fiber/new (fn [] (halt :tail)) |:halt|)
      got (drive-tail f)]
  (assert (= got :tail) "a tail-position resume delivers the halted value")
  (assert (= (fiber/status f) :dead) "the tail-driven child is dead"))
(println "  2. ok")

# ── 3. Nested fibers (the resume trampoline) ───────────────────────
#
# The inner fiber halts; the outer fiber catches it and returns the value. The
# outer resume therefore unwinds through the trampoline rather than driving the
# halting child directly.

(println "  3. halt through a nested resume...")
(let [inner (fiber/new (fn [] (halt :inner)) |:halt|)
      outer (fiber/new (fn [] (fiber/resume inner)) |:halt|)
      got (fiber/resume outer)]
  (assert (= got :inner) "the halted value crosses both fiber boundaries")
  (assert (= (fiber/status inner) :dead) "the halting child is dead")
  (assert (= (fiber/status outer) :dead)
          "the outer fiber completed, delivering the value"))
(println "  3. ok")

# ── 4. A halted fiber does not resume ──────────────────────────────
#
# Dead is terminal. This is what makes the release at halt time safe: nothing
# can re-enter the fiber to touch what was freed.

(println "  4. a halted fiber refuses a second resume...")
(let [f (fiber/new (fn [] (halt :once)) |:halt|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :dead) "dead after the first resume")
  (let [[ok? _] (protect (fiber/resume f))]
    (assert (not ok?) "resuming a dead fiber is an error")))
(println "  4. ok")

# ── 5. Halt without a value ────────────────────────────────────────
#
# `(halt)` carries nil. The fiber still ends dead — the finalization keys on
# the signal, not on the payload.

(println "  5. halt with no value...")
(let [f (fiber/new (fn [] (halt)) |:halt|)
      got (fiber/resume f)]
  (assert (nil? got) "a bare halt delivers nil")
  (assert (= (fiber/status f) :dead) "a bare halt still ends the fiber"))
(println "  5. ok")

# ── 6. Repeated halts keep halting ─────────────────────────────────
#
# The halt finalization runs once per fiber and leaves the driver able to run
# the next one. Fifty in a row must each deliver their own value.

(println "  6. repeated halts...")
(var n 0)
(while (< n 50)
  (let [f (fiber/new (fn [] (halt n)) |:halt|)]
    (assert (= (fiber/resume f) n) "each fiber delivers its own value")
    (assert (= (fiber/status f) :dead) "each fiber halts and dies"))
  (assign n (+ n 1)))
(println "  6. ok")

(println "  all fiber-halt tests passed")
