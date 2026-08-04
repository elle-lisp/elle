(elle/epoch 12)
# squelch-fuel: a fuel pause passes through squelch and attune boundaries.
#
# :fuel is not program behavior. The VM injects it at the interpreter's
# charge sites to meter a fiber, and the metering parent owns the pause:
# lib/process.lisp preemption and the debugger's step engine both deliver
# it under arbitrary user code. So the enforcement chokepoints exempt the
# pause bits (`signals::squelched_bits`, src/signals/mod.rs) and a boundary
# that names :fuel is inert — metering is the parent's action, not the
# closure's behavior, so the boundary has nothing to enforce. The pause
# propagates to the metering parent as plain :fuel with a nil payload, and
# a refueled resume runs the body to completion.
#
# A boundary that converts the pause into a signal-violation instead
# discards the suspended frames and leaves the fiber :paused and resumable
# over the wreckage, so the resume runs the interrupted instruction against
# a torn-down stack. The two charge-site shapes tear down different state,
# hence one case each: a TailCall site (S1) and a backward jump (S2). The
# interpreter and the JIT call paths share one predicate, so the assertions
# hold identically under either tier.

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

# S4: the exemption is for the pause bits alone. A boundary that names
# both :fuel and a user signal keeps converting the user signal.
(def yielder
  (fn []
    (yield 1)
    7))

(let [f (fiber/new (fn [] ((squelch tail-looper |:fuel :yield|)))
                   |:fuel :yield :error|)]
  (fiber/set-fuel f 3)
  (fiber/resume f)
  (assert (= (fiber/status f) :paused) "S4: the fiber pauses at the meter")
  (assert (= (fiber/value f) nil)
          "S4: the boundary passes the :fuel pause through")
  (fiber/set-fuel f 100000)
  (fiber/resume f)
  (assert (= (fiber/status f) :dead) "S4: the refueled fiber completes")
  (assert (= (fiber/value f) 45) "S4: the boundary did not perturb the result"))

(let [caught (try
               ((squelch yielder |:fuel :yield|))
               (catch e (get e :error)))]
  (assert (= caught :signal-violation)
          "S4: the same boundary still converts the squelched :yield"))

(println "squelch-fuel: ok")
