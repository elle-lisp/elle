(elle/epoch 12)
# plumb.lisp — the io leak dashboard: every probe whose drive reaches the io
# backend. oracle.lisp is the pure region dashboard and owns the discipline
# this file follows — the estimator, the gauge-live discriminator rule, the
# defect/by-design split, and the completeness gate (oracle.lisp's header;
# lib/estimator.lisp is the shared instrument). Split from the oracle because
# io probes need axes it lacks: fixtures with setup and teardown, wall-clock
# tolerance in their own epsilons, the backend as a run dimension, and later a
# descriptor gauge — while an io fixture failure must not void the pure
# dashboard's verdicts. Every probe here is read on the object count AND the
# region count in one drive (`measure-2`): io machinery moves whole region
# entries between fibers, the scheduler, and requests, so the region dimension
# is representable for every shape in the file and no dual-read table is
# needed until a probe diverges.

# The estimator, the gauges, the ledger, and the `check` macro — spliced at
# compile time, so this dashboard compiles its own copy with fresh ledger
# state.
(include-file "lib/estimator.lisp")

# This process's gauges must prove live on their own — a discriminator's
# verdict never carries across processes.
(each l in ["discriminator (live-growth)" "region discriminator (live-growth)"]
  (put by-design l true))

# ── Discriminators ────────────────────────────────────────────────────
(def @disc-sink @[])
(defn probe-disc [j]
  (push disc-sink {:k j}))
(def @region-disc-sink @[])
(defn probe-region-disc [j]
  (push region-disc-sink {:k j}))

(println "── plumb: io leak dashboard ──")
(def disc (measure "discriminator (live-growth)" probe-disc 200 6 60 0.4 0.5))
(show disc)
(check (assert (= (get disc :verdict) :open)
               (string "GAUGE DEAD: discriminator read " (get disc :verdict)
                       " — every 'closed' verdict this run is void")))
(def region-disc
  (measure-core "region discriminator (live-growth)"
                (fn [b] (run-thunk-block probe-region-disc b)) region-gauge 200
                6 60 0.4 0.5))
(show region-disc)
(check (assert (= (get region-disc :verdict) :open)
               (string "REGION GAUGE DEAD: region discriminator read "
                       (get region-disc :verdict)
                       " — every region-gauge 'closed' verdict this run is "
                       "void")))

# ── The pumped round trip ─────────────────────────────────────────────
# A yielding io op, the whole round trip: ev/sleep is the clean shape
# (portless, nil result), so what the gauge sees is the scheduler pump's own
# per-op cost. The op suspends with an IoRequest, the pump reads a completion
# out of `(io/wait backend …)` and resumes the fiber with it, and every region
# on that path is released — the request's park retain at the resume, the
# completion array and the structs it carries at the pump's own
# `DecrefValueRegion`, both being one region (docs/impl/region/ctx.md § "A
# helper reached from inside a call allocates through THAT call's ctx").
# Measured with the B-invariance self-test, so a reading of 0 is a per-op rate
# rather than a per-block artifact.
(defn probe-io-yield [j]
  (ev/sleep 0))
(def io (measure-stable "io-yield ev/sleep" probe-io-yield 200 8 80 0.4 0.5))
(show io)
(check (assert (not= (get io :verdict) :contaminated)
               (string "io-yield rate is block-dependent (B vs 2B): "
                       (get io :rate) " vs " (get io :alt-rate)
                       " — a per-block artifact, not a per-op rate")))
(check (assert (= (get io :verdict) :closed)
               (string "io-yield leaked: " (get io :verdict) " rate="
                       (get io :rate))))

# ── The displaced io park ─────────────────────────────────────────────
# The three exits of a parked io op. Its `IoRequest` is the RUNTIME's value —
# the native built it and the body names it nowhere — so no continuation
# releases it and whatever ends the park owes that release
# (docs/impl/region/owner.md § "Park/unpark symmetry"). `io-drop` is the exit
# with no install at all, covered by the free-path discharge; `io-abort` and
# `io-refuse` each end the park by raising at the fiber's own suspension point.
# The three must stay together: `io-drop` removes the displacing install, so the
# gap between it and either displacing route isolates the install's owed release
# from the park itself. The per-install leak gauge is
# tests/elle/region-io-park.lisp and the guardfree face is
# tests/elle/region-io-park-uaf.lisp; these read the same shapes as rates.
(defn mk-io []
  (fiber/new (fn []
               (let [r (ev/sleep 10000)]
                 5)) |:io :error|))
(defn probe-io-drop [j]
  (let [f (mk-io)]
    (fiber/resume f)
    3))
(defn probe-io-abort [j]
  (let [f (mk-io)]
    (fiber/resume f)
    (fiber/abort f "no")))
(defn probe-io-refuse [j]
  (let [f (mk-io)]
    (fiber/resume f)
    (fiber/refuse f "no")))
(defn pin-io-2 [label probe opin rpin]
  (let [[r rr] (measure-2 label (fn [b] (run-thunk-block probe b)) count-gauge
                          0.4 0.5 "regions" region-gauge 0.4 0.5 100 6 60)]
    (pin r opin)
    (pin rr rpin)))
(pin-io-2 "io-drop" probe-io-drop 0 0)
(pin-io-2 "io-abort" probe-io-abort 0 0)
(pin-io-2 "io-refuse" probe-io-refuse 0 0)

# ── The split headline ────────────────────────────────────────────────
(println "── split ──")
(def split-tally (stats))
(println "open defects: " split-tally:defects " across " split-tally:roots
         " roots; by-design: " split-tally:by-design
         (if (= (length split-tally:unclassified) 0)
           ""
           (string "; UNCLASSIFIED: " (length split-tally:unclassified) " "
                   split-tally:unclassified)))
(check (assert (= (length split-tally:unclassified) 0)
               (string "unclassified open probe(s): " split-tally:unclassified
                       " — every open probe must be a declared root or "
                       "by-design (the split ledger is stale)")))
(check (assert (= split-tally:by-design 2)
               (string "by-design tally " split-tally:by-design
                       " ≠ 2 — the object-count and region live-growth "
                       "discriminators must each read open")))

(report)
(println "plumb: ok")
