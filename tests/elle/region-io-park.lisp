(elle/epoch 12)
# An io park's `IoRequest` is released by the install that displaces it
# (docs/impl/region/owner.md § "Park/unpark symmetry").
#
# A yielding io op returns its `IoRequest` with `SIG_IO`, and the suspend retains
# the region it lives in so the scheduler can read the request out of
# `fiber.signal`. The request is the RUNTIME's value, not the body's: the native
# built it, the body never named it, and no `decref_point` names its region — so
# the continuation past the suspend releases nothing for it. The reference the
# allocation left is owed by whatever ends the park. A fiber that never runs again
# takes the discard discharge; a fiber whose park is REPLACED owes the release to
# the install that replaces it, and there are three of those: a resume delivering
# the completion, a refusal raising at the fiber's own call site, and an abort
# ending it where it stopped.
#
# The trap: the resume's release carries a skip — a `Fresh` io op (`port/read`,
# `accept`) builds its completion buffer IN the request's region and hands that
# buffer back as the resume value, so there the region is still live and the
# resumer's own release of the result answers for it. That skip is a fact about a
# DELIVERY. An injected error is not one, so the skip must not travel to the
# refusal and the abort, which is what the counter-factual for this file is: leave
# them with the resume's release and every park they end strands its request.
#
# This file is the LEAK gauge — an `arena/count` delta over a fixed window, which
# must be BOUNDED for every install. The soundness complement is
# region-io-park-uaf.lisp; the in-flight face, where the scheduler has already
# submitted the request an abort ends, is `region_fiber_abort_io_protect_uaf`.

(def window 300)

(defn measure (thunk warm window)
  (var i 0)
  (while (%lt i warm)
    (thunk)
    (assign i (%add i 1)))
  (def before (arena/count))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  (%sub (arena/count) before))

# A body that parks on a timer nothing will ever fire. The mask catches `:io` so
# the park comes back to this driver as data rather than reaching a scheduler,
# and `:error` so a refusal the body does not catch is data too.
(defn io-body ()
  (let [r (ev/sleep 10000)]
    5))

(defn mk ()
  (fiber/new io-body |:io :error|))

# subjects — the three displacing installs ─────────────────────────────────────

# (a) abort: the fiber is ended where it stopped.
(defn w-abort ()
  (let [f (mk)]
    (fiber/resume f)
    (fiber/abort f :done)
    (fiber/status f)))

# (b) refuse: the refusal is raised at the fiber's own call site.
(defn w-refuse ()
  (let [f (mk)]
    (fiber/resume f)
    (fiber/refuse f :denied)
    (fiber/status f)))

# (c) resume: the mediator answers the io call with a completion value. Already
# bounded before the abort and refusal faces were, so it is the reading that says
# the other two are the install's strand and not the park's.
(defn w-resume ()
  (let [f (mk)]
    (fiber/resume f)
    (fiber/resume f 7)))

# (d) the park displaced twice — the body catches its own refusal and parks again,
# so the second install must account exactly like the first.
(defn twice-body ()
  (let [[ok1? e1] (protect (ev/sleep 10000))
        [ok2? e2] (protect (ev/sleep 10000))]
    (if ok1? 1 (if ok2? 2 0))))
(defn w-twice ()
  (let [f (fiber/new twice-body |:io :error|)]
    (fiber/resume f)
    (fiber/refuse f :first)
    (fiber/refuse f :second)
    (fiber/value f)))

# (e) the park inside a `protect`, which runs the body in an INNER fiber. The
# request parks there and the outer fiber awaits it through a `FiberResume` frame,
# so the install reaches a second route into the same rule.
(defn protect-body ()
  (let [[ok? e] (protect (ev/sleep 10000))]
    (if ok? 1 0)))
(defn mk-protect ()
  (fiber/new protect-body |:io :error|))
(defn w-protect-abort ()
  (let [f (mk-protect)]
    (fiber/resume f)
    (fiber/abort f :done)
    (fiber/status f)))
(defn w-protect-refuse ()
  (let [f (mk-protect)]
    (fiber/resume f)
    (fiber/refuse f :denied)
    (fiber/value f)))
(defn w-protect-resume ()
  (let [f (mk-protect)]
    (fiber/resume f)
    (fiber/resume f 7)))

# controls ─────────────────────────────────────────────────────────────────────

# (f) the io park nobody displaces: the discard discharge releases it, so this
# face was already bounded and proves the strand is the install's.
(defn c-cold ()
  (let [f (mk)]
    (fiber/resume f)
    3))

# (g) hard kill — the teardown route, likewise already bounded.
(defn c-cancel ()
  (let [f (mk)]
    (fiber/resume f)
    (fiber/cancel f :gone)
    3))

# (h) an ordinary `emit` park displaced by the same installs. Its payload IS
# body-allocated, so the body's own continuation release answers for it and the
# install owes nothing — an io release that reached this park would free the
# payload under its reader.
(defn emit-body ()
  (let [r (emit :yield {:a 1})]
    5))
(defn mk-emit ()
  (fiber/new emit-body |:yield :error|))
(defn c-emit-abort ()
  (let [f (mk-emit)]
    (fiber/resume f)
    (fiber/abort f :done)
    (fiber/status f)))
(defn c-emit-refuse ()
  (let [f (mk-emit)]
    (fiber/resume f)
    (fiber/refuse f :denied)
    (fiber/status f)))

(def abort-d (measure w-abort 60 window))
(def refuse-d (measure w-refuse 60 window))
(def resume-d (measure w-resume 60 window))
(def twice-d (measure w-twice 60 window))
(def protect-abort-d (measure w-protect-abort 60 window))
(def protect-refuse-d (measure w-protect-refuse 60 window))
(def protect-resume-d (measure w-protect-resume 60 window))
(def cold-d (measure c-cold 60 window))
(def cancel-d (measure c-cancel 60 window))
(def emit-abort-d (measure c-emit-abort 60 window))
(def emit-refuse-d (measure c-emit-refuse 60 window))

(println "region-io-park deltas over " window " iters:")
(println "  abort " abort-d "  refuse " refuse-d "  resume " resume-d "  twice "
         twice-d)
(println "  through protect: abort " protect-abort-d "  refuse "
         protect-refuse-d "  resume " protect-resume-d)
(println "  controls: cold " cold-d "  cancel " cancel-d "  emit+abort "
         emit-abort-d "  emit+refuse " emit-refuse-d)

# Every strand in this class is one whole `IoRequest` per ended park, so a
# survivor reads ≥300 over the window. 60 is slack for the one-time intercept.
(defn bounded? (d label)
  (assert (%lt d 60) (concat label " leaks, delta=" (number->string d))))

(bounded? cold-d "control: an io park nobody displaces")
(bounded? cancel-d "control: an io park hard-killed")
(bounded? emit-abort-d "control: an emit park ended by an abort")
(bounded? emit-refuse-d "control: an emit park ended by a refusal")
(bounded? resume-d "io request displaced by a resume")
(bounded? abort-d "io request displaced by an abort")
(bounded? refuse-d "io request displaced by a refusal")
(bounded? twice-d "two io parks refused in one session")
(bounded? protect-abort-d "io park inside a `protect`, ended by an abort")
(bounded? protect-refuse-d "io park inside a `protect`, ended by a refusal")
(bounded? protect-resume-d "io park inside a `protect`, ended by a resume")

# Value preservation: the release must not change what the mediation reads.
(assert (= (w-abort) :error) "an aborted io park did not end :error")
(assert (= (w-refuse) :error) "a refusal the body does not catch is an error")
(assert (= (w-resume) 5) "a resumed io park lost the body's result")
(assert (= (w-twice) 0) "both refusals should have failed their calls")
(assert (= (w-protect-refuse) 0)
        "a refusal through `protect` did not fail the call")
(assert (= (w-protect-resume) 1)
        "a resume through `protect` did not answer the call")

(println "region-io-park: ok")
