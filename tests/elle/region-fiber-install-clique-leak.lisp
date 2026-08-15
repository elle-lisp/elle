(elle/epoch 12)
# The fiber value installers declare `Delivers`, not `Mixed`.
#
# `fiber/resume`, `fiber/abort`, `fiber/cancel` and `fiber/emit` each hand a
# value to another fiber by installing it in that fiber's signal slot, AND each
# returns a value the caller cannot name (whatever the delivered-to fiber hands
# back). `Mixed` is the one declaration that carries both properties, and it
# pays for the result half with the arg clique: a mutual `IncrefRegion` between
# every pair of heap arguments, balanced only by a store's free-time cascade.
#
# No install owes that incref. Every seam accounts for its own reference at
# runtime — an install that outlives the call takes the park-retain and records
# the `fiber -> signal` outgoing edge, and an install the next step consumes is
# a transient handover the caller's own parked frame keeps alive — so a
# compile-time incref never balances and the pair leaks per call.
# `Delivers { args }` answers the two properties separately: no clique on the
# argument side (`Funnel`'s answer), a fiber-frontier escape seed for the listed
# args (`Sends`'s answer), and an unbounded result (`Opaque`'s answer).
# See docs/impl/region/effects.md.
#
# Each face below churns one installer with a HEAP payload, which is what arms
# the clique — a pair needs two heap arguments, so an immediate payload measures
# nothing. The bound is bounded-growth over 200 iterations, and the counterpart
# correctness face reads the delivered payload back after the caller's own
# reference is gone: the seam's retain, not the clique's, is what keeps it live.

# ── Gauge liveness ───────────────────────────────────────────────────────
# A module-level sink that genuinely retains every fiber it makes. If this
# reads bounded, the region gauge is dead and every bound below is void.
(def keep @[])

(defn keeper-churn [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (push keep (fiber/new (fn [] 1) 0))
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(assert (%gt (keeper-churn 200) 100)
        "gauge is dead: a module-level sink of 200 fibers must grow the region count")

# ── fiber/resume delivers its resume value ───────────────────────────────
(defn resume-churn [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (let [f (fiber/new (fn []
                         (yield 1)
                         2) |:yield|)]
      (fiber/resume f)
      (fiber/resume f [1 2 3]))
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(let [d (resume-churn 200)]
  (assert (%lt d 30)
          (string "fiber/resume install clique: 200 iters grew the region count by "
                  d " (the resume value's install owes no compile-time incref)")))

# ── fiber/abort delivers its error payload ───────────────────────────────
(defn abort-churn [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (let [f (fiber/new (fn []
                         (yield 1)
                         2) |:yield :error|)]
      (fiber/resume f)
      (fiber/abort f [1 2 3]))
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(let [d (abort-churn 200)]
  (assert (%lt d 30)
          (string "fiber/abort install clique: 200 iters grew the region count by "
                  d
                  " (the injected error's install owes no compile-time incref)")))

# ── fiber/cancel delivers its error payload ──────────────────────────────
(defn cancel-churn [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (let [f (fiber/new (fn []
                         (yield 1)
                         2) |:yield|)]
      (fiber/resume f)
      (fiber/cancel f [1 2 3]))
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(let [d (cancel-churn 200)]
  (assert (%lt d 30)
          (string "fiber/cancel install clique: 200 iters grew the region count by "
                  d
                  " (the kill payload's park-retain is the only reference it owes)")))

# ── fiber/emit delivers its emitted value ────────────────────────────────
# Both arguments are heap here — the mask is a keyword set — so the clique has a
# pair to emit even though the payload never leaves the emitting fiber.
(defn emit-churn [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (let [f (fiber/new (fn []
                         (fiber/emit |:yield| [1 2 3])
                         2) |:yield|)]
      (fiber/resume f)
      (fiber/resume f))
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(let [d (emit-churn 200)]
  (assert (%lt d 30)
          (string "fiber/emit install clique: 200 iters grew the region count by "
                  d
                  " (the emitted value's SuspendEscape retain is its only reference)")))

# ── Correctness: a delivered payload outlives the caller's reference ─────
# The clique incref used to pin every payload region for the process's life, so
# these reads could not distinguish a live seam from a leak. Each one drops the
# caller's own binding and then reads the payload back out of the fiber.

# The resumed body sees the delivered value.
(let [f (fiber/new (fn [] (+ (yield 1) 10)) |:yield|)]
  (fiber/resume f)
  (assert (= (fiber/resume f 32) 42)
          "fiber/resume delivers its resume value to the suspended body"))

# A killed fiber parks the payload as its terminal value, readable afterward.
(let [f (fiber/new (fn []
                     (yield 1)
                     2) |:yield|)]
  (fiber/resume f)
  (fiber/cancel f [1 2 3])
  (assert (= (fiber/value f) [1 2 3])
          "fiber/cancel parks its payload as the killed fiber's terminal value")
  (assert (= (fiber/status f) :error) "a cancelled fiber is :error"))

# An abort of an already-dead fiber hands back a value read OUT of its fiber
# argument — the result-alias face of the unbounded result side.
(let [f (fiber/new (fn [] [7 8 9]) 0)]
  (fiber/resume f)
  (assert (= (fiber/abort f :ignored) [7 8 9])
          "aborting a completed fiber hands back its terminal value"))

# The emitted payload survives the emitting body's own release: the resumer
# reads it after control has left the emitting activation.
(let [f (fiber/new (fn []
                     (fiber/emit |:yield| [1 2 3])
                     2) |:yield|)]
  (fiber/resume f)
  (assert (= (fiber/value f) [1 2 3])
          "fiber/emit's payload survives the emitting activation's release"))
