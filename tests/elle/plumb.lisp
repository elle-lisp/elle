(elle/epoch 12)
# audited: 2026-09-20
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
# The over-free gate, opened here and closed after the last probe — the same two
# reads oracle.lisp makes, and the same argument for them (oracle.lisp § "The
# over-free gate"). The io backend moves whole region entries between fibers,
# the scheduler, and requests, so a release that runs twice is as reachable here
# as anywhere and this file's probes are not covered by the oracle's gate.
(def over-frees-before (arena/over-frees))
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
# (docs/impl/region/park.md). `io-drop` is the exit
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
(defn pin-io-2-at [label probe opin rpin block minb maxb]
  (let [[r rr] (measure-2 label (fn [b] (run-thunk-block probe b)) count-gauge
                          0.4 0.5 "regions" region-gauge 0.4 0.5 block minb maxb)]
    (pin r opin)
    (pin rr rpin)))
(defn pin-io-2 [label probe opin rpin]
  (pin-io-2-at label probe opin rpin 100 6 60))
(pin-io-2 "io-drop" probe-io-drop 0 0)
(pin-io-2 "io-abort" probe-io-abort 0 0)
(pin-io-2 "io-refuse" probe-io-refuse 0 0)

# ── The abort the scheduler routes ────────────────────────────────────
# `io-abort` above ends a park through `fiber/abort` with no scheduler in the
# picture. `ev/abort` reaches the same primitive through the event loop, which
# then RECORDS what it did — a status record and a mark, both keyed by the
# fiber (docs/scheduler.md § "Completion records") — and CANCELS the operation
# the target was parked in, which leaves an entry holding its operands until
# the completion arrives (docs/impl/io-inflight.md § "A cancelled operation
# reads nothing again"). Either holder keeps the fiber, its closure, and the
# payload the abort delivered, per call, for as long as the loop runs.
#
# The two probes must stay together, and neither subsumes the other.
# `ev-abort` isolates the abort: it yields once so the target is genuinely
# parked in io, then aborts it and nothing else. `ev-timeout` is the shape the
# documentation tells a caller to bound work with, and it never blocks on io at
# all — its body wins at once — so it is the one that reads what an unreaped
# cancel costs a loop that gives the backend no chance to reap.
(defn probe-ev-abort [j]
  (let [f (ev/spawn (fn [] (ev/sleep 30)))]
    (ev/sleep 0)
    (ev/abort f)))
(defn probe-ev-timeout [j]
  (ev/timeout 30 (fn [] j)))
(pin-io-2 "ev-abort" probe-ev-abort 0 0)
(pin-io-2 "ev-timeout" probe-ev-timeout 0 0)

# ── The answer a completion BUILDS ────────────────────────────────────
# `io-yield ev/sleep` above answers with nil, so its completion builds nothing
# and the whole round trip costs the request's region alone. These two answer
# with a value the completion had to BUILD, because nothing could reserve it
# before the operation finished: a `subprocess` whose pid the spawn decides, and
# the bytes a `read-all` has only once the stream ends. Such a value is born in
# a region the completion owns and hands over as it becomes a value
# (docs/impl/io-inflight.md § "A completion owns what it builds").
#
# The three must stay together. `io-yield` removes the built answer and reads
# the same 0, so the gap between it and either of these is the whole of what a
# completion's own region costs — one region and its objects per call, linear in
# the calls a program makes, and invisible to every other probe in this file.
#
# `subprocess-exec` runs at a tenth of the block size the rest of the file uses:
# a block here is a block of CHILD PROCESSES, and the estimator's stopping rule
# converges on a deterministic shape in its minimum blocks either way.
(def built-dir (file/mktempdir))
(def built-path (string built-dir "/plumb-read-all"))
(spit built-path "plumb")
(defn probe-subprocess-exec [j]
  (let [p (subprocess/exec "/bin/sh" ["-c" ":"]
                           {:stdin :null :stdout :null :stderr :null})]
    (subprocess/wait p)))
(defn probe-read-all [j]
  (let [p (port/open built-path :read)
        s (port/read-all p)]
    (port/close p)
    (length s)))
(pin-io-2-at "subprocess-exec" probe-subprocess-exec 0 0 10 6 40)
(pin-io-2 "port-read-all" probe-read-all 0 0)
(delete-file built-path)
(delete-directory built-dir)

# The over-free gate closes here, over every probe above and the load before it.
(def over-frees-after (arena/over-frees))
(check (assert (= over-frees-after 0)
               (string "over-free: " over-frees-after
                       " direct double-release(s) this process, "
                       (- over-frees-after over-frees-before)
                       " of them across the probes — a release that ran twice, "
                       "which no leak rate can see "
                       "(docs/impl/region/diagnostics.md)")))

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
