(elle/epoch 9)
# ── Capability enforcement tests ───────────────────────────────────────
#
# Tests for the "capabilities down" model: fiber/new :deny, fiber/caps,
# and runtime enforcement of withheld capabilities.
#
# Note: arithmetic ops (+, -, etc.) are compiled to specialized bytecode
# instructions that bypass call_inner. Use non-specialized primitives
# like `length`, `type-of`, `car` for enforcement tests.

# ── Phase 1: Infrastructure ───────────────────────────────────────────

# fiber/caps returns a keyword set with all capabilities for the root fiber
(let [caps (fiber/caps)]
  (assert (set? caps) "fiber/caps returns a set")
  (assert (caps :error) "root fiber has :error capability")
  (assert (caps :io) "root fiber has :io capability")
  (assert (caps :ffi) "root fiber has :ffi capability")
  (assert (caps :exec) "root fiber has :exec capability"))

# fiber/caps on a fiber argument — no deny = full caps
(let [f (fiber/new (fn [] 42) |:error|)]
  (let [caps (fiber/caps f)]
    (assert (set? caps) "fiber/caps on new fiber returns a set")
    (assert (caps :error) "new fiber has :error cap")
    (assert (caps :io) "new fiber has :io cap")))

# :deny creates a fiber with withheld capabilities
(let [f (fiber/new (fn [] (fiber/caps)) |:error| :deny |:io|)]
  (let [result (fiber/resume f)]
    (assert (set? result) "child fiber/caps returns a set")
    (assert (result :error) "child has :error (not denied)")
    (assert (not (result :io)) "child lacks :io (denied)")
    (assert (result :ffi) "child has :ffi (not denied)")))

# fiber/caps on a denied fiber — external view matches internal view
(let [f (fiber/new (fn [] 42) |:error| :deny |:io :ffi|)]
  (let [caps (fiber/caps f)]
    (assert (not (caps :io)) "external: child lacks :io")
    (assert (not (caps :ffi)) "external: child lacks :ffi")
    (assert (caps :error) "external: child has :error")))

# Narrowing composes transitively
(let [outer (fiber/new (fn []
                         (let [inner (fiber/new (fn [] (fiber/caps)) |:error|
                                 :deny |:ffi|)]
                           (fiber/resume inner))) |:error| :deny |:io|)]
  (let [result (fiber/resume outer)]
    (assert (not (result :io)) "grandchild lacks :io (from parent)")
    (assert (not (result :ffi)) "grandchild lacks :ffi (from own deny)")
    (assert (result :error) "grandchild has :error")))

# Widening is silently absorbed (child can't gain caps parent lacks)
(let [outer (fiber/new (fn []
                         (let [inner (fiber/new (fn [] (fiber/caps)) |:error|)]
                           (fiber/resume inner))) |:error| :deny |:io|)]
  (let [result (fiber/resume outer)]
    (assert (not (result :io)) "grandchild inherits :io denial from parent")))

# ── Phase 2: Enforcement ─────────────────────────────────────────────

# :deny |:error| blocks length (non-specialized, declares Signal::errors)
(let [f (fiber/new (fn [] (length "hello")) |:error| :deny |:error|)]
  (let [result (fiber/resume f)]
    (assert (= (fiber/status f) :paused) "fiber paused after denial")
    (let [val (fiber/value f)]
      (assert (= (val :error) :capability-denied) "denial payload")
      (assert (set? (val :denied)) ":denied is a set")
      (assert ((val :denied) :error) ":denied set contains :error")
      (assert (= (val :primitive) "length") "denial names blocked primitive"))))

# :deny |:io| blocks port/read-line (declares SIG_IO)
(let [f (fiber/new (fn [] (port/read-line (*stdin*))) |:io :error| :deny |:io|)]
  (let [result (fiber/resume f)]
    (assert (= (fiber/status f) :paused) "fiber paused after IO denial")
    (let [val (fiber/value f)]
      (assert (= (val :error) :capability-denied) "IO denial payload")
      (assert ((val :denied) :io) ":denied set contains :io"))))

# Mediation: parent catches denial, inspects payload
(let [f (fiber/new (fn [] (length "hello")) |:error| :deny |:error|)]
  (let [denial (fiber/resume f)]
    (assert (= (fiber/status f) :paused) "child paused after denial")
    (let [val (fiber/value f)]
      (assert (= (val :primitive) "length") "blocked primitive is length")
      (assert (= (first (val :args)) "hello") "args captured correctly"))))

# Mask-interaction: denied + masked = parent catches denial
(let [f (fiber/new (fn [] (length "x")) |:error| :deny |:error|)]
  (let [result (fiber/resume f)]
    (assert (= (fiber/status f) :paused) "denied+masked: parent catches")))

# Mask-interaction: denied + not masked = denial propagates
(let [outer (fiber/new (fn []
                         (let [inner (fiber/new (fn [] (length "x")) 0
                                 :deny |:error|)]
                           (fiber/resume inner))) |:error|)]
  (let [result (fiber/resume outer)]
    (assert (= (fiber/status outer) :paused) "denial propagates to grandparent")))

# Nesting: two levels of deny compose
(let [outer (fiber/new (fn []
                         (let [inner (fiber/new (fn [] (length "x")) |:error|
                                 :deny |:error|)]
                           (fiber/resume inner))) |:error| :deny |:io|)]
  (let [result (fiber/resume outer)]
    (assert (= (fiber/status outer) :dead) "outer completes normally")
    (assert (= (result :error) :capability-denied)
      "inner denial as return value")))

# No-deny fibers work exactly as before
(let [f (fiber/new (fn [] (length "hello")) |:error|)]
  (assert (= (fiber/resume f) 5) "no-deny fiber runs length normally"))
