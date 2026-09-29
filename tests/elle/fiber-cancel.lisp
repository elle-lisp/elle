(elle/epoch 13)
# audited: 2026-09-28
# Fiber cancel and propagate: the value each delivers, and the states that refuse it.
# tests/AGENTS.md
# docs/signals/fibers.md

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

## ── cancel leaves the fiber in error status ─────────────────────────

(let [f (fiber/new (fn [] 42) 1)]
  (fiber/cancel f "never started")
  (assert (= (string (fiber/status f)) "error")
          "fiber cancel: new fiber becomes error"))

(let [f (fiber/new (fn []
                     (yield "waiting")
                     99) 3)]
  (fiber/resume f)
  (fiber/cancel f "stop")
  (assert (= (string (fiber/status f)) "error")
          "cancel: a suspended fiber becomes error"))

(let [f (fiber/new (fn [] (emit :error 99)) 1)]
  (fiber/resume f)
  (fiber/cancel f "cancelling suspended")
  (assert (= (string (fiber/status f)) "error")
          "cancel: a fiber paused on a caught error becomes error"))

## ── cancel refuses a fiber that is done ─────────────────────────────

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

(let [[ok? _] (protect ((fn []
                          (let [f (fiber/new (fn [] (emit :error 99)) 0)]
                            (let [wrapper (fiber/new (fn [] (fiber/resume f)) 1)]
                              (fiber/resume wrapper)
                              (fiber/cancel f "already errored"))))))]
  (assert (not ok?) "cancel rejects errored fiber"))

## ── cancel in tail position ─────────────────────────────────────────

(let [target (fiber/new (fn [] 42) 1)]
  (let [canceller (fiber/new (fn [] (fiber/cancel target "cancelled")) 0)]
    (fiber/resume canceller)
    (assert (= (string (fiber/status target)) "error")
            "fiber cancel: tail position")))

(let [target (fiber/new (fn []
                          (yield 0)
                          99) 3)]
  (fiber/resume target)
  (let [canceller (fiber/new (fn [] (fiber/cancel target "stop")) 0)]
    (fiber/resume canceller)
    (assert (= (string (fiber/status target)) "error")
            "fiber cancel suspended: tail position")))

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
