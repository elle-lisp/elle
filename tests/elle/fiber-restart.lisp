(elle/epoch 13)
# audited: 2026-09-28
# A restart answers the call that raised.
#
# A fiber stopped on an error parks just past the raising call, and
# `fiber/resume` restarts it there: the resume value takes that call's
# result, and the frames that called it run on. This file pins that for
# every raise site — `error`, a primitive, an instruction, a callee, an
# injected abort, and a child's error that escaped into its parent. Most
# cases stop a caught (:paused) fiber, and one stops an uncaught (:error) one.
#
# Two counter-factuals. A primitive or an instruction that raises pushes a nil
# where its result goes. A park that keeps the nil makes a restart push the
# resume value on top of it, and the call's consumer reads the nil. And a
# replay that drops the frames outside a raising frame makes the restart answer
# the inner call and end the fiber with that value.
#
# docs/signals/primitives.md

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

# The replayed frame raises at the same call, so it parks again, and the
# next restart answers that call.
(let [f (fiber/new (fn [] (list :got (+ 1 (get nil :x)))) |:error|)]
  (fiber/resume f)
  (assert (= :type-error (get (fiber/resume f nil) :error))
          "a nil restart raises in +")
  (assert (= (list :got 42) (fiber/resume f 41))
          "and the next restart answers +"))

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

# The callee's frames are gone by the time the fiber stops, so the restart
# answers the body's call to it (docs/signals/primitives.md).
(defn raise-now []
  (+ 100 (error :boom)))
(let [f (fiber/new (fn [] (list :got (raise-now))) |:error|)]
  (fiber/resume f)
  (assert (= (list :got 41) (fiber/resume f 41))
          "the body's call to the callee answers the restart"))

# The parameterize bindings the callee's frames made leave with them. The
# counter-factual is a binding that stays: the body reads the callee's 9, and
# its own scope-end pop takes the callee's frame, so 5 outlives its scope.
(def restart-depth (make-parameter 0))
(defn bound-raise []
  (parameterize ((restart-depth 9))
    (+ 1 (get nil :x))))
(let [f (fiber/new (fn []
                     (list (parameterize ((restart-depth 5))
                             (list (bound-raise) (restart-depth)))
                           (restart-depth))) |:error|)]
  (fiber/resume f)
  (assert (= (list (list 41 5) 0) (fiber/resume f 41))
          "a callee's binding leaves with its abandoned frames"))

# The same for a callee whose parameterize itself raised. The flag keeps the
# callee from being inferred silent, which a raise in it would violate.
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
  (assert (= (list (list 41 5) 0) (fiber/resume f 41))
          "and the body's own bindings are the ones it runs on with"))

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
# stray that a later instruction consumes in place of its own operand.

(defn apply-silent [f x]
  (silence f)
  (list :got (f x)))
(let [f (fiber/new (fn [] (apply-silent (fn [x] (yield x)) 41)) |:error :yield|)]
  (assert (= :signal-violation (get (fiber/resume f) :error))
          "a silence bound raises")
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

# The object limit raises between two instructions, after the allocating one
# pushed its result. The trap: the limit is heap-wide, so the fiber clears it
# itself, and the resumer allocates nothing between the raise and the restart.
(def over-limit
  (fiber/new (fn []
               (arena/set-object-limit (+ (arena/count) 3))
               (let [xs (list 1 2 3 4 5 6 7 8)]
                 (arena/set-object-limit nil)
                 (list :len (length xs)))) |:error|))
(def limit-error (fiber/resume over-limit))
(def limit-restart (fiber/resume over-limit :ignored))
(assert (= :allocation-error (get limit-error :error)) "the object limit raises")
(assert (= (list :len 8) limit-restart)
        "the restart keeps the allocation the limit raised after")

(println "fiber-restart: OK")
