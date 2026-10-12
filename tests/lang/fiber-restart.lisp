(elle/epoch 15)
# audited: 2026-10-06
# A restart answers the call that raised, for every raise site, and continues past a raise that has no result.
# docs/signals/primitives.md
#
# A fiber stopped on an error parks just past the raising call, and
# `fiber/resume` restarts it there: the resume value takes that call's result,
# and the frames that called it run on. This file pins that for every raise
# site — `error`, a primitive, an instruction, a callee at any depth, an
# injected abort, a denied call, and a child's error that escaped into its
# parent — and pins the raises with no result, which a restart continues past.
# Most cases stop a caught (:paused) fiber, and one stops an uncaught (:error)
# one.
# tests/impl/fiber-restart-object-limit.lisp holds the object limit's face.
#
# Three counter-factuals. A primitive or an instruction that raises pushes a nil
# where its result goes. A park that keeps the nil makes a restart push the
# resume value on top of it, and the call's consumer reads the nil. A replay
# that drops the frames outside a raising frame makes the restart answer the
# inner call and end the fiber with that value. And a raise that abandons the
# callee it left makes the restart answer the caller's call to that callee.

# ── The raise site ────────────────────────────────────────────────────

# `error` is an `Emit`, which leaves no placeholder, so it is the control.
(let [f (fiber/new (fn [] (list :got (+ 100 (error :boom)))) |:error|)]
  (assert (= :boom (fiber/resume f)) "the raise is caught")
  (assert (= (fiber/status f) :paused) "and the fiber waits")
  (assert (= (list :got 141) (fiber/resume f 41)) "error answers the restart"))

(let [f (fiber/new (fn [] (list :got (+ 1 (get nil :x)))) |:error|)]
  (assert (= :type-error (get (fiber/resume f) :error)) "a primitive raises")
  (assert (= (list :got 42) (fiber/resume f 41))
          "the restart value answers the primitive's call"))

(let [f (fiber/new (fn [] (list :got (first nil))) |:error|)]
  (fiber/resume f)
  (assert (= (list :got 7) (fiber/resume f 7))
          "the restart value answers a raising instruction"))

(let [f (fiber/new (fn []
                     (list :got (match 5
                                  1 :one))) |:error|)]
  (fiber/resume f)
  (assert (= (list :got :five) (fiber/resume f :five))
          "and a match that no arm covers"))

# The operands below the raising call are the caller's, and a restart must
# leave every one of them where it was.
(let [f (fiber/new (fn [] (list :a :b (get nil :x) :c)) |:error|)]
  (fiber/resume f)
  (assert (= (list :a :b 7 :c) (fiber/resume f 7))
          "the restart value lands between the operands"))

# ── An :error fiber restarts the same way ─────────────────────────────

(let [f (fiber/new (fn [] (list :got (+ 1 (get nil :x)))) |:yield|)]
  (assert (not (first (protect (fiber/resume f)))) "the error passes the resume")
  (assert (= (fiber/status f) :error) "and the fiber stops :error")
  (assert (= (list :got 42) (fiber/resume f 41)) "a restart answers the call"))

# ── A restart that raises again ───────────────────────────────────────

# A nil restart answers `get`, and `double` raises on the nil. `double` is a
# callee the replay enters, so its raise parks it behind the replayed frame,
# and the next restart answers the raising `first` inside it.
(defn double [v]
  (* 2 (first v)))
(let [f (fiber/new (fn [] (list :got (double (get nil :x)))) |:error|)]
  (fiber/resume f)
  (assert (= :type-error (get (fiber/resume f nil) :error))
          "a nil restart raises in double")
  (assert (= (list :got 40) (fiber/resume f 20))
          "and the next restart answers the first inside double"))

# ── A callee that suspended before it raised ──────────────────────────

(defn yield-then-raise []
  (yield 1)
  (+ 100 (error :boom)))
(defn yield-then-get []
  (yield 1)
  (+ 100 (get nil :x)))

(let [f (fiber/new (fn [] (list :got (yield-then-raise))) |:yield :error|)]
  (assert (= 1 (fiber/resume f)) "the callee yields")
  (assert (= :boom (fiber/resume f)) "then raises")
  (assert (= (list :got 141) (fiber/resume f 41))
          "the restart runs the callee and the body that called it"))

(let [f (fiber/new (fn [] (list :got (yield-then-get))) |:yield :error|)]
  (fiber/resume f)
  (fiber/resume f)
  (assert (= (list :got 141) (fiber/resume f 41))
          "and the same for a primitive raise in the callee"))

# ── A callee that raised on the fiber's first run ─────────────────────

# An error parks every frame from the raise to the body, so the restart answers
# the raising call inside the callee, and the callee and the body run on
# (docs/signals/primitives.md).
(defn raise-now []
  (+ 100 (error :boom)))
(let [f (fiber/new (fn [] (list :got (raise-now))) |:error|)]
  (fiber/resume f)
  (assert (= (list :got 141) (fiber/resume f 41))
          "the error inside the callee answers the restart"))

(defn get-now []
  (+ 100 (get nil :x)))
(let [f (fiber/new (fn [] (list :got (get-now))) |:error|)]
  (fiber/resume f)
  (assert (= (list :got 141) (fiber/resume f 41))
          "and the get inside the callee, for a primitive raise"))

# Three frames deep, through a tail call, and through a destructure: each
# parks every frame between the raise and the body.
(defn outer-call []
  (+ 1000 (raise-now)))
(let [f (fiber/new (fn [] (list :got (outer-call))) |:error|)]
  (fiber/resume f)
  (assert (= (list :got 1141) (fiber/resume f 41))
          "every frame between the raise and the body runs on"))

(defn via-tail []
  (raise-now))
(let [f (fiber/new (fn [] (list :got (via-tail))) |:error|)]
  (fiber/resume f)
  (assert (= (list :got 141) (fiber/resume f 41))
          "a tail call parks the callee it entered"))

(defn take-pair [x]
  (def [a b] x)
  (list :pair a b))
(let [f (fiber/new (fn [] (list :got (take-pair 7))) |:error|)]
  (assert (= :type-error (get (fiber/resume f) :error))
          "the destructure raises in the callee")
  (assert (= :type-error (get (fiber/resume f [1 2]) :error))
          "the restart answers the first element, and the second raises")
  (assert (= (list :got (list :pair [1 2] 3)) (fiber/resume f 3))
          "each element answers its own restart, as in a body"))

# The parameterize bindings the parked frames made stay in force. The
# counter-factual is a binding dropped at the raise: the callee's restart reads
# 5, the body's binding, in place of its own 9.
(def restart-depth (make-parameter 0))
(defn bound-raise []
  (parameterize ((restart-depth 9))
    (+ (restart-depth) (get nil :x))))
(let [f (fiber/new (fn []
                     (list (parameterize ((restart-depth 5))
                             (list (bound-raise) (restart-depth)))
                           (restart-depth))) |:error|)]
  (fiber/resume f)
  (assert (= (list (list 50 5) 0) (fiber/resume f 41))
          "the callee's binding stays in force across the restart"))

# A callee whose parameterize itself raised binds nothing, and the restart
# continues past the raise inside the callee. The flag keeps the callee from
# being inferred silent, which a raise in it would violate.
(def not-a-param 42)
(defn bad-binding [flag]
  (when flag (error :never))
  (parameterize ((not-a-param 1))
    :x))
(let [f (fiber/new (fn []
                     (list (parameterize ((restart-depth 5))
                             (list (bad-binding false) (restart-depth)))
                           (restart-depth))) |:error|)]
  (assert (= :type-error (get (fiber/resume f) :error))
          "the callee's parameterize raises")
  (assert (= (list (list :x 5) 0) (fiber/resume f 41))
          "and the callee runs on past it, the restart value going nowhere"))

# An :error fiber restarts inside the callee the same way.
(let [f (fiber/new (fn [] (list :got (get-now))) |:yield|)]
  (assert (not (first (protect (fiber/resume f)))) "the error passes the resume")
  (assert (= (list :got 141) (fiber/resume f 41))
          "and the restart answers the get inside the callee"))

# ── A denied call in a callee ─────────────────────────────────────────

# A denial of :error parks like any denial, so the resume answers the denied
# call inside the callee, and the callee's caller runs on.
(defn measured [s]
  (def n (length s))
  (list :measured n))
(let [f (fiber/new (fn []
                     (def r (measured "hello"))
                     (list :body r)) |:error| :deny |:error|)]
  (fiber/resume f)
  (assert (fiber/denied? f) "the length call is denied")
  (assert (= (list :body (list :measured 5)) (fiber/resume f 5))
          "the resume answers length, and both frames run on"))

# ── An abort raises at the suspension point ───────────────────────────

(let [f (fiber/new (fn [] (list :got (+ 100 (yield 1)))) |:yield :error|)]
  (fiber/resume f)
  (assert (= :stop (fiber/abort f :stop)) "the abort answers its error")
  (assert (= (fiber/status f) :error) "and leaves the fiber :error")
  (assert (= (list :got 141) (fiber/resume f 41))
          "the restart answers the yield the abort raised at"))

(defn yields-one []
  (+ 100 (yield 1)))
(let [f (fiber/new (fn [] (list :got (yields-one))) |:yield :error|)]
  (fiber/resume f)
  (fiber/abort f :stop)
  (assert (= (list :got 141) (fiber/resume f 41))
          "and in a callee, the body that called it runs on"))

# The fiber's own protect sees the aborted call raise.
(let [f (fiber/new (fn []
                     (let [[ok? err] (protect (yield 1))]
                       (list ok? err))) |:yield :error|)]
  (fiber/resume f)
  (fiber/abort f :stop)
  (assert (= (fiber/status f) :dead)
          "the protect catches and the fiber completes")
  (assert (= (list false :stop) (fiber/value f)) "holding the abort error"))

# ── A parent a child's error escaped into ─────────────────────────────

# The child's mask lets the error pass, so it stops the parent at the
# parent's own fiber/resume call; a restart of the parent answers that call.
(let [p (fiber/new (fn []
                     (let [c (fiber/new (fn [] (error :boom)) |:yield|)]
                       (list :parent (fiber/resume c)))) |:error|)]
  (assert (= :boom (fiber/resume p)) "the child's error reaches the root")
  (assert (= (list :parent 7) (fiber/resume p 7))
          "the restart answers the parent's fiber/resume"))

# ── A raise with no result ────────────────────────────────────────────

# A raise with no result position has nothing to answer: a restart continues
# after it, and the restart value goes nowhere. The counter-factual is a
# restart that pushes its value anyway, which lands on the operand stack as a
# stray that a later instruction consumes in place of its own operand. The
# yielding closure sits in an array, so the compiler cannot see its signal
# and leaves the bound to the entry check.

(defn apply-silent [f x]
  (bound f ||)
  (list :got (f x)))
(def hidden-yield (get @[(fn [x] (yield x))] 0))
(let [f (fiber/new (fn [] (apply-silent hidden-yield 41)) |:error :yield|)]
  (assert (= :signal-violation (get (fiber/resume f) :error)) "a bound raises")
  (assert (= 41 (fiber/resume f :ignored))
          "the restart continues past the check")
  (assert (= (list :got 7) (fiber/resume f 7)) "and the body runs on"))

(def depth (make-parameter 0))
(def not-a-parameter 42)
(let [f (fiber/new (fn []
                     (parameterize ((depth 5))
                       (list :a (parameterize ((not-a-parameter 1))
                                  :x) (depth)))) |:error|)]
  (assert (= :type-error (get (fiber/resume f) :error)) "parameterize raises")
  (assert (= (list :a :x 5) (fiber/resume f :ignored))
          "the restart runs the body with the bindings around it"))

(println "fiber-restart: OK")
