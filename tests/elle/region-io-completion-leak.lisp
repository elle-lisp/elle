(elle/epoch 12)
# Counterfactual for the io-completion region leak (oracle.lisp's
# `io-yield ev/sleep` probe, focused).
#
# `io/wait` and `io/reap` declare `RegionEffect::Fresh`: everything the call
# allocates is born in the call's own result region, so the caller's one
# `DecrefValueRegion` on the returned array reclaims the whole result. A
# completion struct built in a region of its OWN is not covered by that
# release — the array's counted `array ⊇ struct` edge is the struct region's
# only other holder, so the array's free cascade drops it to its own birth
# reference and stops. One struct region strands per completion, which is one
# per yielding io op: unbounded in any long-running io loop.
#
# The scheduler pump is the shape that hits it: every `(ev/sleep …)`,
# `(port/read …)` or `(ev/poll-fd …)` suspends with an IoRequest, and the pump
# hands the fiber back a completion out of `(io/wait backend …)`.

# ── Leak bound: a pumped io op strands no region ───────────────────────
# Each iteration suspends on a zero-duration timer and is resumed from a
# completion. A correct runtime frees the completion array and its structs
# together at the pump's `DecrefValueRegion`, so live growth over N iterations
# does not scale with N.
(defn sleep-churn [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (ev/sleep 0)
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(let [d500 (sleep-churn 500)]
  (assert (%lt d500 50)
          (string "io completion region leak: 500 pumped sleeps grew the region "
                  "count by " d500 " (must stay bounded — Rule 8)")))

# The same property at object granularity: the completion struct itself is the
# leaked object, so the live-object count must not scale either.
(defn sleep-object-churn [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (ev/sleep 0)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d500 (sleep-object-churn 500)]
  (assert (%lt d500 50)
          (string "io completion object leak: 500 pumped sleeps grew the live "
                  "count by " d500 " (must stay bounded — Rule 8)")))

# ── Correctness: the completion still carries its payload ──────────────
# A completion's `:value` field points at a value the backend built in its own
# region, so the struct records a cross-region edge to it. Reading that value
# after the pump has released the completion array proves the edge is counted
# rather than the struct merely outliving its holder.
(with-temp-dir tmp
               (let [path (string tmp "/payload.txt")]
                 (file/write path "completion")
                 (let [p (port/open path :read)]
                   (let [chunk (port/read p 10)]
                     (assert (= chunk "completion")
                             "a port read resumed from a completion returns its payload"))
                   (port/close p))))

# A spawned fiber resumed from a completion still returns its own result, and
# the scheduler reclaims the whole spawn/join round.
(assert (= (ev/join (ev/spawn (fn []
                                (ev/sleep 0)
                                42))) 42)
        "a spawned fiber resumed from an io completion returns its result")
