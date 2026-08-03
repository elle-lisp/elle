(elle/epoch 12)
# squelch-fuel: a fuel pause must pass through squelch and attune boundaries.
#
# THIS TEST FAILS, identically under both tiers. It goes green when the
# enforcement chokepoints exempt the VM's pause bits (docs/debugger.md
# § "The :debug signal", "Transparent to signal hygiene").
#
# :fuel is not program behavior. The VM injects it at the interpreter's
# charge sites to meter a fiber, and the metering parent owns the pause:
# lib/process.lisp preemption and the debugger's step engine both deliver
# it under arbitrary user code. `enforce_squelch` (src/vm/core.rs) exempts
# only :error, :halt, and :switch, so a boundary that names :fuel converts
# the pause into a signal-violation instead. The conversion discards the
# suspended frames — and leaves the fiber :paused and resumable over the
# wreckage. Observed today when the violation is caught and the fiber is
# resumed:
#
#   - a TailCall charge site resumes into a Rust panic
#     ("VM bug: Stack underflow on TailCall");
#   - a backward-jump charge site resumes into nil-filled locals
#     ("+: expected number, got nil").
#
# Correct behavior, asserted below: the pause propagates to the metering
# parent as plain :fuel with a nil payload, and a refueled resume runs the
# body to completion. A squelch of :fuel is then inert — metering is the
# parent's action, not the closure's behavior, so the boundary has nothing
# to enforce.

(def tail-looper
  (fn []
    (letrec [loop (fn (n acc) (if (%lt n 10) (loop (%add n 1) (%add acc n)) acc))]
      (loop 0 0))))

(def jump-looper
  (fn []
    (var n 0)
    (var acc 0)
    (while (< n 10)
      (assign acc (+ acc n))
      (assign n (+ n 1)))
    acc))

# S1: squelch :fuel around tail-call charge sites.
(let [f (fiber/new (fn [] ((squelch tail-looper :fuel))) |:fuel :error|)]
  (fiber/set-fuel f 3)
  (fiber/resume f)
  (assert (= (fiber/status f) :paused) "S1: the fiber pauses at the meter")
  (assert (= (fiber/value f) nil)
          "S1: the boundary passes the :fuel pause through (nil payload)")
  (fiber/set-fuel f 100000)
  (fiber/resume f)
  (assert (= (fiber/status f) :dead) "S1: the refueled fiber completes")
  (assert (= (fiber/value f) 45) "S1: the boundary did not perturb the result"))

# S2: squelch :fuel around backward-jump charge sites.
(let [f (fiber/new (fn [] ((squelch jump-looper :fuel))) |:fuel :error|)]
  (fiber/set-fuel f 3)
  (fiber/resume f)
  (assert (= (fiber/status f) :paused) "S2: the fiber pauses at the meter")
  (assert (= (fiber/value f) nil)
          "S2: the boundary passes the :fuel pause through (nil payload)")
  (fiber/set-fuel f 100000)
  (fiber/resume f)
  (assert (= (fiber/status f) :dead) "S2: the refueled fiber completes")
  (assert (= (fiber/value f) 45) "S2: the boundary did not perturb the result"))

# S3: attune's allow-mask must exempt the pause bits the same way.
(let [f (fiber/new (fn [] ((attune |:error| tail-looper))) |:fuel :error|)]
  (fiber/set-fuel f 3)
  (fiber/resume f)
  (assert (= (fiber/status f) :paused) "S3: the fiber pauses at the meter")
  (assert (= (fiber/value f) nil)
          "S3: the allow-mask passes the :fuel pause through (nil payload)")
  (fiber/set-fuel f 100000)
  (fiber/resume f)
  (assert (= (fiber/status f) :dead) "S3: the refueled fiber completes")
  (assert (= (fiber/value f) 45) "S3: the boundary did not perturb the result"))

(println "squelch-fuel: ok")
