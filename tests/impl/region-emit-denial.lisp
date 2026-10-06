(elle/epoch 15)
# audited: 2026-10-06
# A denied emit's payload is released by the install that displaces it, and its resume mints nothing.
# docs/impl/region/park.md
#
# An emit of a withheld bit parks a denial struct the runtime built, so no
# continuation of the body releases it: the install that displaces the park
# owes that release, as for a denied call. But the park is an `Emit` park, whose
# continuation funds its own release of the resume value, so the resume owes no
# mint, unlike a denied call.
#
# Two wrong accountings, two counter-factuals. A park recorded as a denied CALL
# mints a reference for every resume value that nothing releases. A park
# recorded as a plain emit releases the struct nowhere, and with it the
# emitted value its `:args` hold. Either strands at least one region per
# mediated emit, so the window below reads ≥400 where it should read near 0.
#
# This file is the LEAK gauge. The soundness complement is
# region-emit-denial-uaf.lisp.

(signal :region-emit-denial/beat)

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

(defn denial? [v]
  (and (struct? v) (= :capability-denied (get v :error))))

# The emitted value is body-allocated, so a stranded denial pins it too.
(defn denied-body ()
  (let [r (emit :region-emit-denial/beat @[1 2])]
    (if (nil? r) 0 5)))

(defn mk ()
  (fiber/new denied-body |:region-emit-denial/beat :error|
             :deny |:region-emit-denial/beat|))

# subjects ─────────────────────────────────────────────────────────────────────

(defn w-resume ()
  (let [f (mk)]
    (fiber/resume f)
    (fiber/resume f @[7])))

(defn w-read-resume ()
  (let [f (mk)]
    (fiber/resume f)
    (let [p (fiber/value f)]
      (fiber/resume f (length (get p :args))))))

(defn w-refuse ()
  (let [f (mk)]
    (fiber/resume f)
    (fiber/refuse f :denied)
    (fiber/status f)))

(defn w-abort ()
  (let [f (mk)]
    (fiber/resume f)
    (fiber/abort f :done)
    (fiber/status f)))

(defn protect-body ()
  (let [[ok? e] (protect (emit :region-emit-denial/beat @[1 2]))]
    (if ok? 1 0)))
(defn mk-protect ()
  (fiber/new protect-body |:region-emit-denial/beat :error|
             :deny |:region-emit-denial/beat|))
(defn w-protect-resume ()
  (let [f (mk-protect)]
    (fiber/resume f)
    (fiber/resume f @[3])))
(defn w-protect-refuse ()
  (let [f (mk-protect)]
    (fiber/resume f)
    (fiber/refuse f :denied)
    (fiber/value f)))

# An error emit is denied too, and parks rather than unwinding.
(defn error-body ()
  (let [r (error {:error :mine :data @[1 2]})]
    (if (nil? r) 0 5)))
(defn mk-error ()
  (fiber/new error-body |:error| :deny |:error|))
(defn w-error-resume ()
  (let [f (mk-error)]
    (fiber/resume f)
    (fiber/resume f @[7])))

# controls: the parks nobody displaces ────────────────────────────────────────

(defn c-cold ()
  (let [f (mk)]
    (fiber/resume f)
    3))

(defn c-cancel ()
  (let [f (mk)]
    (fiber/resume f)
    (fiber/cancel f :gone)
    3))

# The subjects must be denials, or the gauges measure an ordinary emit park.
(let [f (mk)]
  (assert (denial? (fiber/resume f)) "the emit was not denied"))
(let [f (mk-error)]
  (assert (denial? (fiber/resume f)) "the error emit was not denied"))

(def resume-d (measure w-resume 100 window))
(def read-resume-d (measure w-read-resume 100 window))
(def refuse-d (measure w-refuse 100 window))
(def abort-d (measure w-abort 100 window))
(def protect-resume-d (measure w-protect-resume 100 window))
(def protect-refuse-d (measure w-protect-refuse 100 window))
(def error-resume-d (measure w-error-resume 100 window))
(def cold-d (measure c-cold 100 window))
(def cancel-d (measure c-cancel 100 window))

(println "region-emit-denial deltas over " window " iters:")
(println "  resume " resume-d "  read+resume " read-resume-d "  refuse "
         refuse-d "  abort " abort-d)
(println "  through protect: resume " protect-resume-d "  refuse "
         protect-refuse-d)
(println "  error emit: resume " error-resume-d)
(println "  controls: cold " cold-d "  cancel " cancel-d)

(defn bounded? (d label)
  (assert (%lt d 100) (concat label " leaks, delta=" (number->string d))))

(bounded? cold-d "control: a denied emit nobody displaces")
(bounded? cancel-d "control: a denied emit hard-killed")
(bounded? resume-d "a denied emit's payload displaced by a resume")
(bounded? read-resume-d
          "a denied emit's payload read, then displaced by a resume")
(bounded? refuse-d "a denied emit's payload displaced by a refusal")
(bounded? abort-d "a denied emit's payload displaced by an abort")
(bounded? protect-resume-d
          "a denied emit inside a `protect`, answered by a resume")
(bounded? protect-refuse-d
          "a denied emit inside a `protect`, answered by a refusal")
(bounded? error-resume-d "a denied error emit answered by a resume")

(assert (= (w-resume) 5) "a mediated resume lost the body's result")
(assert (= (w-read-resume) 5) "reading the payload changed the body's result")
(assert (= (w-refuse) :error) "a refusal the body does not catch is an error")
(assert (= (w-abort) :error) "an aborted child did not end :error")
(assert (= (w-protect-resume) 1)
        "a resume through `protect` did not answer the emit")
(assert (= (w-protect-refuse) 0)
        "a refusal through `protect` did not fail the emit")
(assert (= (w-error-resume) 5) "a resume did not answer the denied error")

(println "region-emit-denial: ok")
