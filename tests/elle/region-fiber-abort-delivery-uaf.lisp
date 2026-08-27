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
# Each face guards both counter-factuals. Where nothing mints, the payload's
# region is freed while a fiber and the reader still point into it: run under
# `--trace=guardfree`, where every freed page is PROT_NONE, and each read below
# faults at the dereference. Where two things mint, the region strands once per
# abort — that never faults, so the churn faces gauge it instead.
#
# `region-fiber-install-clique-leak.lisp` is the bounded-growth face of the
# `Delivers` declaration this belongs to.

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
# targets the payload and a second mint would strand it once per abort.
(defn caught-churn [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (let [f (fiber/new (fn []
                         (protect (yield 1))
                         7) |:yield :error|)]
      (fiber/resume f)
      (fiber/abort f [1 2 3]))
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(let [d (caught-churn 200)]
  (assert (%lt d 30)
          (string "abort payload caught in body: 200 iters grew the region count by "
                  d
                  " (the in-body handler's resume result is the only consumer)")))

# The fiber catches the injected error and then raises an error of ITS OWN. That
# raise mints its own delivery, so the abort owes the result nothing: a mint keyed
# on "the abort ended in an error" rather than on the injection strands the second
# payload once per abort.
(defn own-error-churn [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (let [f (fiber/new (fn []
                         (protect (yield 1))
                         (error {:own 1})) |:yield :error|)]
      (fiber/resume f)
      (fiber/abort f [1 2 3]))
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(let [d (own-error-churn 200)]
  (assert (%lt d 30)
          (string "fiber raises its own error after catching the abort: 200 iters grew "
                  "the region count by " d
                  " (that raise minted its own delivery)")))

# The same shape where the fiber re-raises the payload it caught. The value is
# the injected one, so identity alone cannot tell this from an unwound abort —
# but the `error` that re-raised it minted a delivery for it, and a second mint
# strands the payload once per abort.
(defn reraise-churn [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (let [f (fiber/new (fn []
                         (let [r (protect (yield 1))]
                           (error (get r 1)))) |:yield :error|)]
      (fiber/resume f)
      (fiber/abort f [1 2 3]))
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(let [d (reraise-churn 200)]
  (assert (%lt d 30)
          (string "fiber re-raises the injected payload: 200 iters grew the region "
                  "count by " d " (the re-raise minted the delivery)")))

# ── Route 4: a replayed `defer` frame is resumed with the payload ────────
# The abort unwinds the deferred body, hands the payload to the parked
# `fiber/resume` continuation — whose result release consumes the delivery —
# and the cleanup's `fiber/propagate` mints afresh for the re-raise. Neither
# the replay nor the abort's own result may mint on top of that.
(defn defer-churn [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
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
  (%sub (arena/region-count) before))

(let [d (defer-churn 200)]
  (assert (%lt d 30)
          (string "abort unwound through defer: 200 iters grew the region count by "
                  d " (the replay and the propagate each fund one consumer)")))

# ── The record: the aborted fiber's own frame owes its release ───────────
# The fiber holds the very value it is aborted with — handed to it as an owned
# parameter — so its abandoned frame owes that value a release. With the
# delivery minted at the injection the frame's reference funds nothing, so the
# injection records the mint (`Fiber::emit_delivery`) and the abandoned-frame
# walk stops exempting the payload's region. Without the record the frame's
# release stays owed forever: one region per abort.
(defn hold-then-yield [q]
  (yield q)
  2)

(defn held-payload-churn [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (let [p {:a 1}
          f (fiber/new hold-then-yield |:yield :error|)]
      (fiber/resume f p)
      (fiber/abort f p))
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(let [d (held-payload-churn 200)]
  (assert (%lt d 30)
          (string "fiber aborted with a value it already holds: 200 iters grew the "
                  "region count by " d " (its frame's release is still owed)")))

# The pair-control: the same frame, aborted with a DIFFERENT value. Nothing the
# record could reach, so it isolates the record from the walk itself.
(defn other-payload-churn [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (let [p {:a 1}
          f (fiber/new hold-then-yield |:yield :error|)]
      (fiber/resume f p)
      (fiber/abort f {:b 2}))
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(let [d (other-payload-churn 200)]
  (assert (%lt d 30)
          (string "fiber aborted with a value it does not hold: 200 iters grew the "
                  "region count by " d)))

# The other side of the same record: the ABORTING frame. A literal materialized
# straight into the `fiber/abort` argument lives in that frame's slot and nowhere
# else, and the escaping error abandons the frame before its release runs. The
# record travels with the propagating signal (`VM::park_propagating_abort`), so
# the walk runs that release instead of exempting the payload's region.
(defn aborting-frame-churn [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (let [f (fiber/new (fn []
                         (yield 1)
                         2) |:yield|)]
      (fiber/resume f)
      (try
        (begin
          (fiber/abort f {:e 1})
          nil)
        (catch e e)))
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(let [d (aborting-frame-churn 200)]
  (assert (%lt d 30)
          (string "payload materialized into the abort's argument: 200 iters grew the "
                  "region count by " d
                  " (the aborting frame's release is still owed)")))

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
