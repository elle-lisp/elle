(elle/epoch 12)
# ── Refusing a denied call ─────────────────────────────────────────────
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
# keeps running. `handle_fiber_abort_signal` forces :error only on an
# UNCAUGHT error; a refactor that forced it unconditionally would take the
# whole mechanism away, and every assertion here would still be about a
# fiber that merely died in the right order.
#
# The trap: this cannot be written against `:deny |:error|`. The child's
# own `protect` runs primitives that declare `:error`, so denying that bit
# breaks the recovery path the refusal is delivered into and the fiber
# ends :error no matter what. `:fs` is the narrow bit a mediator actually
# withholds (see caps-fs.lisp).

(def root (file/mktempdir))
(def a (path/join root "A"))
(def b (path/join root "B"))

# ── A refused fiber survives and runs on ──────────────────────────────

(let [f (fiber/new (fn []
                     (let [[ok1? e1] (protect (file/write a "x"))
                           [ok2? e2] (protect (file/write b "y"))]
                       (list ok1? ok2? (= e1 :first) (= e2 :second))))
                   |:fs :error| :deny |:fs|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :paused) "the child traps on the first write")
  (fiber/refuse f :first)
  (assert (= (fiber/status f) :paused)
          "a refused child that catches stays alive and reaches its next call")
  (assert (= "file/write" (get (fiber/value f) :primitive))
          "and the next call traps on its own account")
  (assert (= b (get (get (fiber/value f) :args) 0))
          "the second denial names the second path")
  (fiber/refuse f :second)
  (assert (= (fiber/status f) :dead) "and then runs to its own completion")
  (assert (= (list false false true true) (fiber/value f))
          "both writes failed at the call site, each with the parent's reason")
  (assert (not (path/exists? a)) "neither refused write reached the disk")
  (assert (not (path/exists? b)) "including the second"))

# ── The refusal is a failure, not a return value ──────────────────────

# The counter-factual for the whole mechanism: a plain resume is a value
# the child cannot tell from a real result.
(let [f (fiber/new (fn [] (protect (file/write a "x"))) |:fs :error| :deny |:fs|)]
  (fiber/resume f)
  (fiber/resume f :pretend)
  (assert (= [true :pretend] (fiber/value f))
          "a plain resume reads as the call's successful return"))

(let [f (fiber/new (fn [] (protect (file/write a "x"))) |:fs :error| :deny |:fs|)]
  (fiber/resume f)
  (fiber/refuse f :nope)
  (assert (= [false :nope] (fiber/value f))
          "a refusal reads as the call's failure"))

# A mediator refuses with whatever it wants the child to see.
(let [f (fiber/new (fn [] (protect (file/write a "x"))) |:fs :error| :deny |:fs|)]
  (fiber/resume f)
  (fiber/refuse f (struct :error :capability-refused :path a))
  (let [[ok? err] (fiber/value f)]
    (assert (not ok?) "the child sees a failure")
    (assert (= :capability-refused (get err :error))
            "carrying the parent's own structured reason")
    (assert (= a (get err :path)) "including the path it refused")))

# ── An uncaught refusal is an ordinary uncaught error ─────────────────

(let [order @[]
      f (fiber/new (fn []
                     (defer
                       (push order :cleanup)
                       (file/write a "x"))) |:fs :error| :deny |:fs|)]
  (fiber/resume f)
  (fiber/refuse f :fatal)
  (assert (= (fiber/status f) :error) "an uncaught refusal ends the fiber")
  (assert (= [:cleanup] (freeze order))
          "and unwinds it through its defer blocks"))

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

(file/delete-dir-all root)
(println "caps-refuse: OK")
