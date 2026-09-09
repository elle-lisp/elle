(elle/epoch 12)
# audited: 2026-09-08
# oracle.lisp — the single leak-state dashboard for the region memory system.
#
# ── Why this exists ───────────────────────────────────────────────────
# The former leak suite measured a per-iteration rate as a two-point INTEGER
# slope `(big-small)/(nbig-nsmall)`. Integer
# division FLOORS any sub-integer rate to 0 — a leak of 0.3 objects/op (one
# object every ~3 ops) reports as "reclaimed". For a long-running server a
# 0.3/op leak is still unbounded RSS, so the floor is a false negative exactly
# where it matters most. It also forces a fixed, memory-hungry big scale
# (n=10000) to average out noise, even when the signal is clean.
#
# This oracle replaces the integer slope with a REAL-VALUED leak rate plus a
# confidence interval, measured by an adaptive sequential estimator:
#
#   - Sample the heap gauge (arena/count — an Immediate primitive, so reading
#     it allocates nothing and does not perturb the measurement) in BLOCKS of
#     B ops. A block's per-op rate = net objects / B. Block-averaging
#     decorrelates consecutive ops (region-id recycling correlates them) and
#     shrinks the sample range.
#   - Welford-update a running mean and variance over the block rates. The mean
#     IS the leak rate; the variance drives the stopping rule.
#   - Stop when the empirical-Bernstein half-width on the mean falls below a
#     target. EB is variance-adaptive: a deterministic leak (every block rate
#     identical → variance 0, observed range 0) converges at the floor in a
#     few blocks, where the old method always paid 10000 ops; a noisy leak runs
#     until its interval is tight. This is the speed/memory win AND the
#     sub-integer sensitivity in one estimator.
#
# This is a MEASUREMENT INSTRUMENT, not the soundness oracle. The trustworthy
# UAF signal is `--trace=guardfree` under the full stdlib (docs/impl/
# region/diagnostics.md); a tight, confident rate here does not prove the
# absence of a use-after-free, only the size of a leak.
#
# ── The gauge-live discriminator (non-negotiable) ─────────────────────
# A measured rate of ~0 means "reclaimed" ONLY if the gauge actually moves. A
# dead gauge (a sampling bug, a stubbed primitive) also reads ~0 and would
# paint every leak green. So the oracle FIRST measures a known-live-growth
# shape (a genuine unbounded retain) and asserts it reads OPEN. If the
# discriminator is not OPEN, the gauge is dead and EVERY "closed" verdict in
# the run is void — the suite fails loudly rather than lying. A second
# self-test, B-invariance (see `measure-stable`), proves a reported rate is a
# true per-op rate and not a per-block-boundary artifact; together they keep the
# instrument honest about both whether it measures and what the number means.
#
# ── The dual-read rule — where a second dimension is representable ────
# The object count cannot see a region entry that holds no object: a pages-less
# owner node, or a region emptied of objects but pinned by an unbalanced count.
# A probe family whose machinery can strand one is read on the object count AND
# the region count in one drive (`measure-2`). The AUTHORITY for membership is
# the `@dual-read` table, which the completeness gate enforces — an entry with
# no region verdict recorded fails the run, so coverage cannot rot into
# silence. Every other family is object-only: a strand that holds an object
# already moves the object count, and none of their shapes reaches the
# owner-node machinery. Each dimension is its own dashboard line under a
# suffixed label (`label@regions`) with its own pin, declaration, and verdict —
# a probe open on two dimensions is two defects, two verdicts from two
# instruments.
#
# The estimator, the gauges, the ledger, and the `check` macro live in
# lib/estimator.lisp, shared with the io dashboard (plumb.lisp) and spliced
# here at compile time — each dashboard compiles its own copy, so ledger state
# is fresh per process.
(include-file "lib/estimator.lisp")

# ── The probe families ────────────────────────────────────────────────
# Each file below is spliced at compile time, exactly as the estimator is, so
# the whole dashboard is still ONE program reporting in ONE place — the split
# is a reading budget, not a change of shape (DOCUMENTATION.md § Documents).
# Include ORDER is run order, and it is also definition order: a family's
# shapes are defined in the file that drives them, and the ledger comes first
# because `pin` classifies against it.
(include-file "probe/ledger.lisp")
(include-file "probe/gauge.lisp")
(include-file "probe/shape.lisp")
(include-file "probe/frame.lisp")
(include-file "probe/factory.lisp")
(include-file "probe/park.lisp")
(include-file "probe/direct.lisp")
(include-file "probe/concurrent.lisp")
(include-file "probe/tailcall.lisp")
(include-file "probe/stdlib.lisp")
(include-file "probe/abort.lisp")
(include-file "probe/store.lisp")
(include-file "probe/yield.lisp")
(include-file "probe/container.lisp")
(include-file "probe/branch.lisp")
(include-file "probe/native.lisp")

# ── The split headline — the number §1's protocol reads, printed by the tool ──
# `open defects` is the burndown count; `by-design` is the fixed growth set; `roots` is
# how many of the six declared roots still have an open probe (it falls to 0
# when the last defect closes). UNCLASSIFIED is appended only when a probe leaked
# without a declaration — a stale ledger, gated below so it can never pass silently.
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
                       " — every open probe must be a declared root or by-design "
                       "(the split ledger is stale)")))
# The dual-read half of the same gate: the table is the coverage authority, so
# an entry that recorded no region verdict this run — a renamed probe, a
# deleted block, a row added without a probe — must fail loudly rather than
# read as coverage the dashboard does not have.
(def @dual-unread @[])
(each l in (keys dual-read)
  (unless (get dual-read-seen l) (push dual-unread l)))
(check (assert (= (length dual-unread) 0)
               (string "dual-read entries with no region verdict this run: "
                       dual-unread
                       " — the coverage table names probes the runner never "
                       "read on the region dimension")))
(check (assert (= split-tally:by-design 5)
               (string "by-design tally " split-tally:by-design
                       " ≠ 5 — the growth probes (the object-count, "
                       "physical-id, region, and bytes live-growth "
                       "discriminators, the sub-integer estimator self-test) "
                       "must each read open")))

(report)
(println "oracle: ok")
