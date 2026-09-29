(elle/epoch 13)
# audited: 2026-09-29
# `fiber/refuse` raises the refusal at a denied child's own call, and the child lives on.
# docs/signals/capabilities.md
#
# A mediator that resumes a denied fiber with an ordinary value tells the
# child the call SUCCEEDED and returned that value. For agent-written code
# that is unsafe: the model writes (file/write path content), reads a
# struct back, and proceeds as though the write landed.
#
# `fiber/refuse` raises the refusal at the child's own call site instead,
# where its `protect` catches it. The contract this file pins is that a
# refusal is not a termination — the child survives and may be refused
# again — because that is what makes refusal usable in a session that
# keeps running. A refused child stops :error only where it lets the error
# pass. The counter-factual stops every refused child :error, and every
# assertion here would then be about a fiber that merely died in the right
# order.
#
# The trap: this cannot be written against `:deny |:error|`. The child's
# own `protect` runs primitives that declare `:error`, so denying that bit
# breaks the recovery path the refusal is delivered into and the fiber
# stops :error no matter what. `:fs` is the narrow bit a mediator actually
# withholds (see tests/lang/caps-fs.lisp).

(defn read-plus-100 [p]
  (+ 100 (file/read p)))

(with-temp-dir root
               (let [a (path/join root "A")
                     b (path/join root "B")]

                 # ── A refused fiber survives and runs on ──────────────────────────

                 (let [f (fiber/new (fn []
                                      (let [[ok1? e1] (protect (file/write a "x"))
                                        [ok2? e2] (protect (file/write b "y"))]
                                        (list ok1? ok2? (= e1 :first)
                                        (= e2 :second)))) |:fs :error|
                                    :deny |:fs|)]
                   (fiber/resume f)
                   (assert (= (fiber/status f) :paused)
                           "the child traps on the first write")
                   (fiber/refuse f :first)
                   (assert (= (fiber/status f) :paused)
                           "a refused child that catches stays alive and reaches its next call")
                   (assert (= "file/write" (get (fiber/value f) :primitive))
                           "and the next call traps on its own account")
                   (assert (= b (get (get (fiber/value f) :args) 0))
                           "the second denial names the second path")
                   (fiber/refuse f :second)
                   (assert (= (fiber/status f) :dead)
                           "and then runs to its own completion")
                   (assert (= (list false false true true) (fiber/value f))
                           "both writes failed at the call site, each with the parent's reason")
                   (assert (not (path/exists? a))
                           "neither refused write reached the disk")
                   (assert (not (path/exists? b)) "including the second"))

                 # ── The refusal is a failure, not a return value ──────────────────

                 # The counter-factual for the whole mechanism: a plain resume is a value
                 # the child cannot tell from a real result.
                 (let [f (fiber/new (fn [] (protect (file/write a "x")))
                                    |:fs :error| :deny |:fs|)]
                   (fiber/resume f)
                   (fiber/resume f :pretend)
                   (assert (= [true :pretend] (fiber/value f))
                           "a plain resume reads as the call's successful return"))

                 (let [f (fiber/new (fn [] (protect (file/write a "x")))
                                    |:fs :error| :deny |:fs|)]
                   (fiber/resume f)
                   (fiber/refuse f :nope)
                   (assert (= [false :nope] (fiber/value f))
                           "a refusal reads as the call's failure"))

                 # A mediator refuses with whatever it wants the child to see.
                 (let [f (fiber/new (fn [] (protect (file/write a "x")))
                                    |:fs :error| :deny |:fs|)]
                   (fiber/resume f)
                   (fiber/refuse f (struct :error :capability-refused :path a))
                   (let [[ok? err] (fiber/value f)]
                     (assert (not ok?) "the child sees a failure")
                     (assert (= :capability-refused (get err :error))
                             "carrying the parent's own structured reason")
                     (assert (= a (get err :path))
                             "including the path it refused")))

                 # ── An uncaught refusal is an ordinary uncaught error ─────────────

                 (let [order @[]
                       f (fiber/new (fn []
                                      (defer
                                        (push order :cleanup)
                                        (file/write a "x"))) |:fs :error|
                                    :deny |:fs|)]
                   (fiber/resume f)
                   (fiber/refuse f :fatal)
                   (assert (= (fiber/status f) :error)
                           "an uncaught refusal stops the fiber :error")
                   (assert (= [:cleanup] (freeze order))
                           "and its defer blocks see the error at the refused call"))

                 # With :error in its mask the mediator catches the refusal itself: the
                 # refuse call answers it, and the stopped child holds it.
                 (let [f (fiber/new (fn [] (file/read a)) |:fs :error|
                                    :deny |:fs|)]
                   (fiber/resume f)
                   (assert (= :no (fiber/refuse f :no))
                           "fiber/refuse answers the refusal")
                   (assert (= (fiber/status f) :error)
                           "the child stops :error at the call")
                   (assert (= :no (fiber/value f)) "and holds the refusal"))

                 # Without :error in its mask, the refusal raises past fiber/refuse into the
                 # mediator, as an error passes fiber/resume.
                 (let [f (fiber/new (fn [] (file/read a)) |:fs| :deny |:fs|)
                       _ (fiber/resume f)
                       [ok? err] (protect (fiber/refuse f :no))]
                   (assert (not ok?) "the refusal raises past fiber/refuse")
                   (assert (= :no err) "carrying the refusal")
                   (assert (= (fiber/status f) :error)
                           "and the child stops :error"))

                 # ── A stopped child restarts at the refused call ──────────────────

                 # The counter-factual is a stale placeholder: a park that keeps the nil
                 # where the denied call's result goes makes a restart push its value on top
                 # of it, so `+` reads the nil and raises.
                 (let [f (fiber/new (fn [] (list :got (+ 100 (file/read a))))
                                    |:fs :error| :deny |:fs|)]
                   (fiber/resume f)
                   (fiber/refuse f :no)
                   (assert (= (list :got 105) (fiber/resume f 5))
                           "the restart value answers the refused call"))

                 # The same call inside a helper. The counter-factual is a lost outer frame:
                 # a restart that answers the helper's call and drops the body that called
                 # it ends the fiber with the helper's 105.
                 (let [f (fiber/new (fn [] (list :got (read-plus-100 a)))
                                    |:fs :error| :deny |:fs|)]
                   (fiber/resume f)
                   (fiber/refuse f :no)
                   (assert (= (list :got 105) (fiber/resume f 5))
                           "the restart runs the helper and the body that called it"))

                 # ── A cancel ends a stopped child for good ────────────────────────

                 (let [ran @[]
                       f (fiber/new (fn []
                                      (file/read a)
                                      (push ran :continued)) |:fs :error|
                                    :deny |:fs|)]
                   (fiber/resume f)
                   (fiber/refuse f :no)
                   (assert (= :gone (fiber/cancel f :gone))
                           "a refused child can be cancelled")
                   (assert (= (fiber/status f) :dead) "and ends :dead")
                   (assert (empty? ran) "its continuation never runs")
                   (assert (not (first (protect (fiber/resume f))))
                           "and it never resumes"))))

# ── Refusal answers a call, so it needs a fiber waiting on one ────────

(let [f (fiber/new (fn [] 42) |:fs :error| :deny |:fs|)
      [ok? err] (protect (fiber/refuse f :nope))]
  (assert (not ok?) "a :new fiber has no call to refuse")
  (assert (= :state-error (get err :error))
          "refusing it is a state error, where fiber/abort would hard-kill it"))

(let [f (fiber/new (fn [] 42) |:fs :error|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :dead) "the fiber completed on its own")
  (let [[ok? err] (protect (fiber/refuse f :nope))]
    (assert (not ok?) "a :dead fiber has no call to refuse")
    (assert (= :state-error (get err :error))
            "refusing it is a state error, where fiber/abort would no-op")))

(println "caps-refuse: OK")
