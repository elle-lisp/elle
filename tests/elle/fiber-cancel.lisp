(elle/epoch 13)
# audited: 2026-09-29
# `fiber/cancel` ends a fiber :dead for good, and `fiber/propagate` re-raises a caught signal.
# tests/AGENTS.md
# docs/signals/primitives.md

## ── cancel delivers its value ───────────────────────────────────────

(let [f (fiber/new (fn [] 42) 1)]
  (let [result (fiber/cancel f 99)]
    (assert (= result 99) "cancel new fiber: result is payload")
    (assert (= (fiber/value f) 99) "cancel new fiber: fiber/value is payload")))

(let [f (fiber/new (fn [] 42) 1)]
  (let [result (fiber/cancel f -50)]
    (assert (= result -50) "cancel new fiber: negative payload")
    (assert (= (fiber/value f) -50) "cancel new fiber: fiber/value negative")))

(let [f (fiber/new (fn []
                     (yield 0)
                     99) 3)]
  (fiber/resume f)
  (let [result (fiber/cancel f 88)]
    (assert (= result 88) "cancel suspended fiber: result is payload")
    (assert (= (fiber/value f) 88)
            "cancel suspended fiber: fiber/value is payload")))

(let [f (fiber/new (fn []
                     (yield 0)
                     99) 3)]
  (fiber/resume f)
  (let [result (fiber/cancel f -25)]
    (assert (= result -25) "cancel suspended fiber: negative payload")
    (assert (= (fiber/value f) -25)
            "cancel suspended fiber: fiber/value negative")))

(let [f (fiber/new (fn [] 42) 1)]
  (fiber/cancel f)
  (assert (= (fiber/value f) nil) "fiber cancel: default nil"))

(let [f (fiber/new (fn [] 42) 1)]
  (cancel f "stopped")
  (assert (= (fiber/value f) "stopped") "cancel alias: works"))

(let [f (fiber/new (fn [] 42) 1)]
  (cancel f)
  (assert (= (fiber/value f) nil) "cancel alias: default nil"))

## ── cancel leaves :dead from every state it accepts ─────────────────

(let [f (fiber/new (fn [] 42) 1)]
  (fiber/cancel f "never started")
  (assert (= (fiber/status f) :dead) "fiber cancel: new fiber becomes dead")
  (assert (= (fiber/bits f) 1) "a cancelled fiber holds SIG_ERROR in its bits"))

(let [f (fiber/new (fn []
                     (yield "waiting")
                     99) 3)]
  (fiber/resume f)
  (fiber/cancel f "cancelled")
  (assert (= (fiber/status f) :dead)
          "fiber cancel: suspended fiber becomes dead"))

# A mask that catches the error leaves the fiber :paused at the raise.
(let [f (fiber/new (fn [] (emit :error 99)) 1)]
  (fiber/resume f)
  (fiber/cancel f "cancelling suspended")
  (assert (= (fiber/status f) :dead)
          "cancel after a caught error: the fiber is dead"))

# A mask that lets the error pass leaves the fiber :error. The counter-factual
# is a cancel that refuses it, which leaves a fiber the program can neither
# restart on purpose nor end.
(let [f (fiber/new (fn [] (emit :error 99)) 0)]
  (let [wrapper (fiber/new (fn [] (fiber/resume f)) 1)]
    (fiber/resume wrapper)
    (assert (= (fiber/status f) :error)
            "an uncaught error leaves the fiber :error")
    (assert (= (fiber/cancel f "gone") "gone") "cancel accepts an :error fiber")
    (assert (= (fiber/status f) :dead) "a cancelled :error fiber is dead")
    (assert (= (fiber/value f) "gone")
            "a cancelled :error fiber holds the payload")))

## ── a cancelled fiber never runs again ──────────────────────────────

(let [@ran false
      f (fiber/new (fn []
                     (yield 0)
                     (assign ran true)
                     99) 3)]
  (fiber/resume f)
  (fiber/cancel f "stop")
  (let [[ok? _] (protect (fiber/resume f))]
    (assert (not ok?) "resume of a cancelled fiber fails"))
  (assert (not ran) "the continuation of a cancelled fiber never runs"))

(let [f (fiber/new (fn []
                     (emit :error 99)
                     :recovered) 1)]
  (fiber/resume f)
  (fiber/cancel f)
  (let [[ok? _] (protect (fiber/resume f))]
    (assert (not ok?) "a cancel after a caught error is not a restart")))

## ── the states cancel refuses ───────────────────────────────────────

(let [[ok? _] (protect ((fn []
                          (let [f (fiber/new (fn [] 42) 0)]
                            (fiber/resume f)
                            (fiber/cancel f "too late")))))]
  (assert (not ok?) "cancel rejects dead fiber (42)"))

(let [[ok? _] (protect ((fn []
                          (let [f (fiber/new (fn [] -100) 0)]
                            (fiber/resume f)
                            (fiber/cancel f "too late")))))]
  (assert (not ok?) "cancel rejects dead fiber (-100)"))

(let [f (fiber/new (fn [] 42) 1)]
  (fiber/cancel f "once")
  (let [[ok? _] (protect (fiber/cancel f "twice"))]
    (assert (not ok?) "cancel rejects a cancelled fiber")))

## ── cancel in tail position ─────────────────────────────────────────

(let [target (fiber/new (fn [] 42) 1)]
  (let [canceller (fiber/new (fn [] (fiber/cancel target "cancelled")) 0)]
    (assert (= (fiber/resume canceller) "cancelled")
            "fiber cancel: tail position answers the payload")))

(let [target (fiber/new (fn []
                          (yield 0)
                          99) 3)]
  (fiber/resume target)
  (let [canceller (fiber/new (fn [] (fiber/cancel target "stop")) 0)]
    (fiber/resume canceller)
    (assert (= (fiber/status target) :dead)
            "fiber cancel suspended: tail position")))

## ── a cancelled task joins as a failure ─────────────────────────────

# The scheduler reads a :dead fiber whose bits hold SIG_ERROR as failed. The
# counter-factual is a scheduler that reads :dead alone, which joins a cancelled
# task as a success holding the cancel payload.
(let* [victim (ev/spawn (fn []
                          (ev/sleep 0.2)
                          :never))
       killer (ev/spawn (fn []
                          (fiber/cancel victim {:error :cancelled :by :killer})))]
  (ev/join killer)
  (assert (= (fiber/status victim) :dead) "the cancelled task is dead")
  (let [[ok? err] (protect (ev/join victim))]
    (assert (not ok?) "joining a cancelled task raises")
    (assert (= (get err :error) :cancelled) "the join raises the cancel payload")
    (assert (= (get err :by) :killer) "the payload is the canceller's own")))

# The process scheduler routes its sub-fibers the same way. The counter-factual
# is a sub-fiber join that reads :dead alone and answers success.
(def process ((import "std/process")))
(def @process-join nil)
(process:start (fn []
                 (let* [victim (ev/spawn (fn []
                          (ev/sleep 0.2)
                          :never))
                        killer (ev/spawn (fn []
                          (fiber/cancel victim {:error :cancelled})))]
                   (ev/join killer)
                   (assign process-join (protect (ev/join victim))))))
(assert (not (first process-join))
        "a cancelled sub-fiber of a process joins as a failure")
(assert (= :cancelled (get (get process-join 1) :error))
        "carrying the cancel payload")

## ── propagate re-raises a caught signal ─────────────────────────────

(let [[ok? _] (protect ((fn []
                          (let [f (fiber/new (fn [] (emit :error 99)) 1)]
                            (fiber/resume f)
                            (fiber/propagate f)))))]
  (assert (not ok?) "propagate re-signals error (99)"))

(let [[ok? _] (protect ((fn []
                          (let [f (fiber/new (fn [] (emit :error -50)) 1)]
                            (fiber/resume f)
                            (fiber/propagate f)))))]
  (assert (not ok?) "propagate re-signals error (-50)"))

(let [inner (fiber/new (fn [] (yield 99)) 2)]
  (let [outer (fiber/new (fn []
                           (fiber/resume inner)
                           (fiber/propagate inner)) 2)]
    (assert (= (fiber/resume outer) 99) "fiber propagate: re-raises a yield")))

## ── propagate keeps the child chain ─────────────────────────────────

(let [inner (fiber/new (fn [] (emit :error "err")) 1)]
  (let [outer (fiber/new (fn []
                           (fiber/resume inner)
                           (fiber/propagate inner)) 1)]
    (fiber/resume outer)
    (assert (= (fiber? (fiber/child outer)) true)
            "fiber propagate: child chain preserved")))

(let [inner (fiber/new (fn [] (yield 99)) 2)]
  (let [outer (fiber/new (fn []
                           (fiber/resume inner)
                           (fiber/propagate inner)) 2)]
    (fiber/resume outer)
    (assert (= (identical? inner (fiber/child outer)) true)
            "fiber propagate: child identity preserved")))

## ── propagate refuses a fiber that finished ─────────────────────────

(let [[ok? _] (protect ((fn []
                          (let [f (fiber/new (fn [] 42) 0)]
                            (fiber/resume f)
                            (fiber/propagate f)))))]
  (assert (not ok?) "propagate rejects dead fiber (42)"))

(let [[ok? _] (protect ((fn []
                          (let [f (fiber/new (fn [] -100) 0)]
                            (fiber/resume f)
                            (fiber/propagate f)))))]
  (assert (not ok?) "propagate rejects dead fiber (-100)"))
