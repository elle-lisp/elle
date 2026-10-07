(elle/epoch 13)
# audited: 2026-09-30
# The estimator's own self-tests: the reclaimed baseline, and the sub-integer rate an integer slope floors to 0.
#
# docs/ratchet.md
# docs/impl/region/diagnostics.md
# The live-growth discriminator per gauge is the instrument's own: it drives
# one ahead of a gauge's first reading and reports it as `<axis> gauge
# (live-growth)`, a growth floor that voids its axis when it reads flat
# (lib/ratchet.md). The two shapes below are the oracle's.

# Bounded shape: an immutable struct built and immediately dropped — the
# reclaimed baseline, at 0.
(defn probe-bounded [j]
  {:x j :y 2})

# Sub-integer growth: one object kept every 3 ops, 0.333/op. An integer slope
# floors this to 0 and calls the shape reclaimed, which for a long-running
# server is still unbounded RSS; the estimator, measured to a tight epsilon,
# reads it at ≈0.33. Its row is a growth floor, so an estimator that stopped
# seeing it voids every object reading in the run.
(def @third-sink @[])
(def @third-ctr 0)
(defn probe-third [j]
  (assign third-ctr (%add third-ctr 1))
  (when (%lt 2 third-ctr)  # ctr reached 3
    (assign third-ctr 0)
    (push third-sink {:k 1})))

(println "── leak oracle ──")
(r:rate "bounded (immutable struct, dropped)" probe-bounded :block 200)
(r:rate "sub-integer (1-in-3 retain)" probe-third :block 300 :min 8 :max 200
        :epsilon 0.05)
