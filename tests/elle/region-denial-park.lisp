(elle/epoch 12)
# A payload the RUNTIME built is released by the install that displaces it
# (docs/impl/region/owner.md § "Park/unpark symmetry").
#
# A park leaves two references on its payload's region: the DELIVERY, which the
# resumer's release of the resume result consumes, and the BODY's own, released
# by the continuation past the suspend. A capability denial has no second one to
# release — the denial path builds `{:error :capability-denied …}` itself, so the
# body never names the value and no `decref_point` names its region. The
# reference the allocation left is owed by whatever displaces the payload.
#
# Three installs displace it, and every one is the mediator's ordinary move:
# `fiber/resume` answers the denied call with a value, `fiber/refuse` raises the
# refusal at the child's own call site, and `fiber/abort` ends the child. Each is
# gauged directly and again through `protect`, which parks the denial in an inner
# fiber and so reaches it by a different route. A fiber that is never resumed
# again takes the discard discharge instead, which is what the `cold` and `cancel`
# controls hold to.
#
# The trap: the payload holds the denied call's ARGUMENTS, so one stranded
# payload pins every heap argument's region behind it. A mediated `file/read`
# strands the struct AND the path string, which is why the object counts here run
# ahead of one per call.
#
# This file is the LEAK gauge — an `arena/count` delta over a fixed window, which
# must be BOUNDED for every displacing install. The soundness complement is
# region-denial-park-uaf.lisp; the discard face is the `denied-discard` probe in
# tests/elle/oracle.lisp.

(def window 400)

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

# A body that traps on its first `:fs` call. The mask catches the denial so the
# parent mediates it; `:error` is masked too, so a refusal the body does not
# catch still comes back as data rather than tearing the driver down.
(defn denied-body ()
  (let [r (file/read "/no/such/path")]
    5))

(defn mk ()
  (fiber/new denied-body |:fs :error| :deny |:fs|))

# subjects — the three displacing installs ─────────────────────────────────────

# (a) resume: the mediator answers the denied call with a value.
(defn w-resume ()
  (let [f (mk)]
    (fiber/resume f)
    (fiber/resume f 7)))

# (b) resume, with the payload read first — the documented mediation idiom
# (tests/elle/caps-fs.lisp). The read is pass-through, so it changes the count by
# nothing and must change the verdict by nothing either.
(defn w-read-resume ()
  (let [f (mk)]
    (fiber/resume f)
    (let [p (fiber/value f)]
      (fiber/resume f (length (get p :primitive))))))

# (c) refuse: the refusal is raised at the child's own call site.
(defn w-refuse ()
  (let [f (mk)]
    (fiber/resume f)
    (fiber/refuse f :denied)
    (fiber/status f)))

# (d) abort: the child is ended where it stopped.
(defn w-abort ()
  (let [f (mk)]
    (fiber/resume f)
    (fiber/abort f :done)
    (fiber/status f)))

# (e) the body catches its own refusal, so the denial parks in the INNER fiber
# `protect` runs it in and the outer one awaits it through a `FiberResume` frame.
# The mediator's install then reaches the inner fiber directly, never through
# `fiber/resume` — a route of its own for both the answer and the refusal.
(defn protect-body ()
  (let [[ok? e] (protect (file/read "/no/such/path"))]
    (if ok? 1 0)))
(defn mk-protect ()
  (fiber/new protect-body |:fs :error| :deny |:fs|))
(defn w-protect-resume ()
  (let [f (mk-protect)]
    (fiber/resume f)
    (fiber/resume f "content")))
(defn w-protect-refuse ()
  (let [f (mk-protect)]
    (fiber/resume f)
    (fiber/refuse f :denied)
    (fiber/value f)))

# (f) two denials in one session — a refused child that catches survives and
# traps again, so the second park must account exactly like the first.
(defn twice-body ()
  (let [[ok1? e1] (protect (file/read "/no/such/one"))
        [ok2? e2] (protect (file/read "/no/such/two"))]
    (if ok1? 1 (if ok2? 2 0))))
(defn w-twice ()
  (let [f (fiber/new twice-body |:fs :error| :deny |:fs|)]
    (fiber/resume f)
    (fiber/refuse f :first)
    (fiber/refuse f :second)
    (fiber/value f)))

# (g) the bits collision. A fiber denied `:io` parks under `SIG_IO` — the very
# bit a yielding io op's `IoRequest` park carries — so at the resume the denial
# payload cannot be told from an io request by its bits, and the two releases
# that could answer for it must be exactly one. This shape carries a per-call
# residual of its own, the rest list the calling convention builds for a variadic
# callee (F2's `denied-discard` rate in tests/elle/oracle.lisp), and its DISCARD
# face carries the identical one — so the pin here is that mediating the denial
# costs no more than dropping the fiber does. Two releases is a use-after-free
# rather than a number, which region-denial-park-uaf.lisp is what catches.
(defn io-denied-body ()
  (println "never runs — the fiber is denied :io"))
(defn mk-io ()
  (fiber/new io-denied-body |:error :io| :deny |:io|))
(defn w-io-resume ()
  (let [f (mk-io)]
    (fiber/resume f)
    (fiber/resume f 7)))
(defn c-io-cold ()
  (let [f (mk-io)]
    (fiber/resume f)
    3))

# controls ─────────────────────────────────────────────────────────────────────

# (h) the denial park nobody displaces: the discard discharge releases it, so
# this face was already bounded and proves the strand is the install's.
(defn c-cold ()
  (let [f (mk)]
    (fiber/resume f)
    3))

# (i) hard kill — the teardown route, likewise already bounded.
(defn c-cancel ()
  (let [f (mk)]
    (fiber/resume f)
    (fiber/cancel f :gone)
    3))

# (j) an ordinary `emit` park displaced the same three ways. Its payload IS
# body-allocated, so the body's own continuation release answers for it and the
# install owes nothing — a release added here would free it under the reader.
(defn emit-body ()
  (let [r (emit :yield {:a 1})]
    5))
(defn mk-emit ()
  (fiber/new emit-body |:yield :error|))
(defn c-emit-resume ()
  (let [f (mk-emit)]
    (fiber/resume f)
    (fiber/resume f 7)))
(defn c-emit-refuse ()
  (let [f (mk-emit)]
    (fiber/resume f)
    (fiber/refuse f :denied)
    (fiber/status f)))

(def resume-d (measure w-resume 100 window))
(def read-resume-d (measure w-read-resume 100 window))
(def refuse-d (measure w-refuse 100 window))
(def abort-d (measure w-abort 100 window))
(def protect-resume-d (measure w-protect-resume 100 window))
(def protect-refuse-d (measure w-protect-refuse 100 window))
(def twice-d (measure w-twice 100 window))
(def io-resume-d (measure w-io-resume 100 window))
(def io-cold-d (measure c-io-cold 100 window))
(def cold-d (measure c-cold 100 window))
(def cancel-d (measure c-cancel 100 window))
(def emit-resume-d (measure c-emit-resume 100 window))
(def emit-refuse-d (measure c-emit-refuse 100 window))

(println "region-denial-park deltas over " window " iters:")
(println "  resume " resume-d "  read+resume " read-resume-d "  refuse "
         refuse-d "  abort " abort-d "  twice " twice-d)
(println "  through protect: resume " protect-resume-d "  refuse "
         protect-refuse-d)
(println "  :io denial (bits collide): resume " io-resume-d "  discard "
         io-cold-d)
(println "  controls: cold " cold-d "  cancel " cancel-d "  emit+resume "
         emit-resume-d "  emit+refuse " emit-refuse-d)

# Every strand in this class is at least one whole payload struct per mediated
# call, so a survivor reads ≥400 over the window. 100 is slack for the one-time
# intercept.
(defn bounded? (d label)
  (assert (%lt d 100) (concat label " leaks, delta=" (number->string d))))

(bounded? cold-d "control: a denial park nobody displaces")
(bounded? cancel-d "control: a denial park hard-killed")
(bounded? emit-resume-d "control: an emit park answered by a resume")
(bounded? emit-refuse-d "control: an emit park answered by a refusal")
(bounded? resume-d "denial payload displaced by a resume")
(bounded? read-resume-d "denial payload read, then displaced by a resume")
(bounded? refuse-d "denial payload displaced by a refusal")
(bounded? abort-d "denial payload displaced by an abort")
(bounded? protect-resume-d "denial inside a `protect`, answered by a resume")
(bounded? protect-refuse-d "denial inside a `protect`, answered by a refusal")
(bounded? twice-d "two denials mediated in one session")

# The `:io` denial carries the variadic callee's own per-call residual on BOTH
# faces, so the pin is relative: mediating must cost no more than dropping.
(assert (%lt io-resume-d (%add io-cold-d 100))
        (concat "a mediated :io denial costs more than a dropped one, "
                (number->string io-resume-d) " vs " (number->string io-cold-d)))

# Value preservation: the release must not change what the mediation reads.
(assert (= (w-resume) 5) "a mediated resume lost the body's result")
(assert (= (w-read-resume) 5) "reading the payload changed the body's result")
(assert (= (w-refuse) :error) "a refusal the body does not catch is an error")
(assert (= (w-abort) :error) "an aborted child did not end :error")
(assert (= (w-protect-resume) 1)
        "a resume through `protect` did not answer the call")
(assert (= (w-protect-refuse) 0)
        "a refusal through `protect` did not fail the call")
(assert (= (w-twice) 0) "both refusals should have failed their calls")

(println "region-denial-park: ok")
