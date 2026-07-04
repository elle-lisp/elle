(elle/epoch 12)
## tests/elle/prim-fibers.lisp
## Fiber creation, resumption, status, fuel management

## ── fiber/new and fiber/status ─────────────────────────────────────

(let [f (fiber/new (fn [] 42) 0)]
  (assert (fiber? f) "fiber/new creates fiber")
  (assert (keyword? (fiber/status f)) "fiber/status returns keyword"))

(let [[ok? _] (protect ((fn [] (fiber/new 42 0))))]
  (assert (not ok?) "fiber/new non-closure errors"))

## ── fiber/resume ───────────────────────────────────────────────────

(let [f (fiber/new (fn [] 42) 0)]
  (fiber/resume f)
  (assert (= (fiber/value f) 42) "fiber/resume runs body"))

(let [[ok? _] (protect ((fn [] (fiber/resume 42))))]
  (assert (not ok?) "fiber/resume non-fiber errors"))

## ── fiber/value ────────────────────────────────────────────────────

(let [[ok? _] (protect ((fn [] (fiber/value 42))))]
  (assert (not ok?) "fiber/value non-fiber errors"))

## ── fiber/status ───────────────────────────────────────────────────

(let [[ok? _] (protect ((fn [] (fiber/status 42))))]
  (assert (not ok?) "fiber/status non-fiber errors"))

## ── fiber/set-fuel ─────────────────────────────────────────────────

(let [f (fiber/new (fn [] 42) 0)]
  (fiber/set-fuel f 1000)
  (assert (= (fiber/fuel f) 1000) "fiber/set-fuel stores value")
  (fiber/set-fuel f 0)
  (assert (= (fiber/fuel f) 0) "fiber/set-fuel zero"))

(let [[ok? _] (protect ((fn [] (fiber/set-fuel 42 100))))]
  (assert (not ok?) "fiber/set-fuel non-fiber errors"))

(let [f (fiber/new (fn [] 42) 0)]
  (let [[ok? _] (protect ((fn [] (fiber/set-fuel f -1))))]
    (assert (not ok?) "fiber/set-fuel negative errors")))

(let [f (fiber/new (fn [] 42) 0)]
  (let [[ok? _] (protect ((fn [] (fiber/set-fuel f :oops))))]
    (assert (not ok?) "fiber/set-fuel non-integer errors")))

## ── fiber/fuel ─────────────────────────────────────────────────────

(let [f (fiber/new (fn [] 42) 0)]
  (assert (nil? (fiber/fuel f)) "fiber/fuel unlimited returns nil")
  (fiber/set-fuel f 42)
  (assert (= (fiber/fuel f) 42) "fiber/fuel returns integer when set"))

(let [[ok? _] (protect ((fn [] (fiber/fuel 42))))]
  (assert (not ok?) "fiber/fuel non-fiber errors"))

## ── fiber/clear-fuel ───────────────────────────────────────────────

(let [f (fiber/new (fn [] 42) 0)]
  (fiber/set-fuel f 100)
  (fiber/clear-fuel f)
  (assert (nil? (fiber/fuel f)) "fiber/clear-fuel removes limit"))

(let [f (fiber/new (fn [] 42) 0)]
  (fiber/clear-fuel f)  # no-op on unlimited
  (assert (nil? (fiber/fuel f)) "fiber/clear-fuel on unlimited is noop"))

(let [[ok? _] (protect ((fn [] (fiber/clear-fuel 42))))]
  (assert (not ok?) "fiber/clear-fuel non-fiber errors"))

(println "prim-fibers: all tests passed")
