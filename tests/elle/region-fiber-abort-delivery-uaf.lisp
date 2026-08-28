(elle/epoch 12)
# `fiber/abort` injects a payload the CALLER owns, so the injection mints the one
# delivery reference every route's consumer releases.
#
# The caller's own reference answers the caller's ARGUMENT release and nothing
# else. Exactly one further release fires on the payload as a RESULT, and which
# one depends on where the injected error stops — the four faces below. One
# reference, one consumer, four routes: `inject_error_at_suspension` mints it
# once at the seam all four leave through
# (docs/impl/region/effects.md § `Delivers`).
#
# This file owns the SOUNDNESS face. Where nothing mints, the payload's region
# is freed while a fiber and the reader still point into it: run under
# `--trace=guardfree`, where every freed page is PROT_NONE, and each read below
# faults at the dereference.
#
# The other direction — where two things mint, and the region strands once per
# abort rather than faulting — is the `abort-*` probe family in
# tests/elle/oracle.lisp, one probe per route and per recorded mint. It measures
# a per-op rate with a confidence interval rather than comparing two heap
# samples, which is what a two-point integer delta over a fixed window cannot do:
# it floors a sub-integer rate to zero. `region-fiber-install-clique-leak.lisp`
# is the bounded-growth face of the `Delivers` declaration this belongs to.

# ── Route 1: the fiber's mask catches, the caller reads the result ───────
# The abort's caller receives the payload back and releases it as a result on
# top of releasing it as an argument.
(let [f (fiber/new (fn []
                     (yield 1)
                     2) |:yield :error|)]
  (fiber/resume f)
  (let [r (fiber/abort f [1 2 3])]
    (assert (= r [1 2 3])
            "fiber/abort hands the injected payload back to its caller")
    (assert (= (get r 2) 3)
            "the payload's contents are readable after the abort")))

# The payload also stays readable through the fiber it was parked on.
(let [f (fiber/new (fn []
                     (yield 1)
                     2) |:yield :error|)]
  (fiber/resume f)
  (fiber/abort f [4 5 6])
  (assert (= (fiber/value f) [4 5 6])
          "the aborted fiber's terminal value is the injected payload"))

# ── Route 2: the error escapes the fiber, an ancestor absorbs it ─────────
# The mask names only `:yield`, so the injected error travels out of the fiber
# to the enclosing `try`, which absorbs it and releases the payload as its
# resume result. The caller still owns the payload across that whole trip and
# reads it afterwards, so an unfunded escape frees it under both readers.
(def @i 0)
(while (%lt i 200)
  (let [payload {:error :injected}
        f (fiber/new (fn []
                       (yield 1)
                       2) |:yield|)]
    (fiber/resume f)
    (let [e (try
              (begin
                (fiber/abort f payload)
                nil)
              (catch e e))]
      (assert (= (get e :error) :injected)
              "the escaped abort payload reaches the catching try intact")
      (assert (= (get payload :error) :injected)
              "the abort's caller still reads the payload it injected")))
  (assign i (%add i 1)))

# ── Route 3: a handler INSIDE the fiber catches the injected error ───────
# The in-body handler's release of its own resume result consumes the delivery,
# and the fiber hands the caller a value of its own — so no caller release
# targets the payload. The abort runs to the fiber's own completion, which is
# where a payload freed one reference short faults.
(def @i 0)
(while (%lt i 200)
  (let [f (fiber/new (fn []
                       (let [r (protect (yield 1))]
                         (assert (= (get (get r 1) 2) 3)
                                 "the in-body handler reads the injected payload")
                         7)) |:yield :error|)]
    (fiber/resume f)
    (assert (= (fiber/abort f [1 2 3]) 7)
            "the caller receives the fiber's own value, not the payload"))
  (assign i (%add i 1)))

# The fiber catches the injected error and then raises an error of ITS OWN. That
# raise mints its own delivery, so the abort owes the result nothing.
(def @i 0)
(while (%lt i 200)
  (let [f (fiber/new (fn []
                       (protect (yield 1))
                       (error {:own 1})) |:yield :error|)]
    (fiber/resume f)
    (assert (= (get (fiber/abort f [1 2 3]) :own) 1)
            "the caller reads the fiber's OWN error, raised after the catch"))
  (assign i (%add i 1)))

# The same shape where the fiber re-raises the payload it caught. The value is
# the injected one, so identity alone cannot tell this from an unwound abort —
# but the `error` that re-raised it minted a delivery for it.
(def @i 0)
(while (%lt i 200)
  (let [payload [1 2 3]
        f (fiber/new (fn []
                       (let [r (protect (yield 1))]
                         (error (get r 1)))) |:yield :error|)]
    (fiber/resume f)
    (assert (= (get (fiber/abort f payload) 2) 3)
            "the re-raised payload reaches the caller intact")
    (assert (= (get payload 0) 1)
            "the abort's caller still reads the payload it injected"))
  (assign i (%add i 1)))

# ── Route 4: a replayed `defer` frame is resumed with the payload ────────
# The abort unwinds the deferred body, hands the payload to the parked
# `fiber/resume` continuation — whose result release consumes the delivery —
# and the cleanup's `fiber/propagate` mints afresh for the re-raise.
(def @i 0)
(while (%lt i 200)
  (let [f (fiber/new (fn []
                       (defer
                         (length [1 2 3 4 5])
                         (yield 1)
                         2)) |:yield :error|)]
    (fiber/resume f)
    (let [r (fiber/abort f [7 8 9])]
      (assert (= (get r 0) 7)
              "the payload survives the aborted body's defer cleanup")))
  (assign i (%add i 1)))

# ── The record: the aborted fiber's own frame owes its release ───────────
# The fiber holds the very value it is aborted with — handed to it as an owned
# parameter — so its abandoned frame owes that value a release. With the
# delivery minted at the injection the frame's reference funds nothing, so the
# injection records the mint (`Fiber::emit_delivery`) and the abandoned-frame
# walk stops exempting the payload's region.
(defn hold-then-yield [q]
  (yield q)
  2)

(def @i 0)
(while (%lt i 200)
  (let [p {:a 1}
        f (fiber/new hold-then-yield |:yield :error|)]
    (fiber/resume f p)
    (fiber/abort f p)
    (assert (= (get p :a) 1)
            "the caller reads a payload the aborted fiber also held"))
  (assign i (%add i 1)))

# The pair-control: the same frame, aborted with a DIFFERENT value. Nothing the
# record could reach, so it isolates the record from the walk itself.
(def @i 0)
(while (%lt i 200)
  (let [p {:a 1}
        f (fiber/new hold-then-yield |:yield :error|)]
    (fiber/resume f p)
    (fiber/abort f {:b 2})
    (assert (= (get p :a) 1)
            "the value the fiber held is untouched by an abort naming another"))
  (assign i (%add i 1)))

# The other side of the same record: the ABORTING frame. A literal materialized
# straight into the `fiber/abort` argument lives in that frame's slot and nowhere
# else, and the escaping error abandons the frame before its release runs. The
# record travels with the propagating signal (`VM::park_propagating_abort`), so
# the walk runs that release instead of exempting the payload's region.
(def @i 0)
(while (%lt i 200)
  (let [f (fiber/new (fn []
                       (yield 1)
                       2) |:yield|)]
    (fiber/resume f)
    (let [e (try
              (begin
                (fiber/abort f {:e 1})
                nil)
              (catch e e))]
      (assert (= (get e :e) 1)
              "the literal materialized into the abort argument reaches the catch")))
  (assign i (%add i 1)))

# ── Churn: repeated abort delivery must not accumulate stale pages ───────
(def @i 0)
(while (%lt i 200)
  (let [f (fiber/new (fn []
                       (yield 1)
                       2) |:yield :error|)]
    (fiber/resume f)
    (assert (= (get (fiber/abort f [1 2 3]) 1) 2)
            "repeated abort delivery keeps the payload live"))
  (assign i (%add i 1)))
