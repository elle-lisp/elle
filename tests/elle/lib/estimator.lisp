(elle/epoch 12)
# audited: 2026-09-17
## The instrument the leak dashboards share: the gauges, the estimator, the
## ledger, and the channel each verdict is reported through.
## docs/impl/region/diagnostics.md
## docs/test-store.md
##
## Each dashboard (oracle.lisp, plumb.lisp) splices this file with the
## top-level `include-file` directive (docs/modules.md § "Compile-Time
## Inclusion"), so every dashboard compiles its own copy: fresh ledger state
## per process — which is why each must run its own gauge-live discriminators
## (oracle.lisp § "The gauge-live discriminator"); a gauge is proven live per
## process, never per library — and the `check` macro crosses, splicing
## preceding expansion. This directory is outside the corpus glob
## (`tests/elle/*.lisp`), so the library is never run as a test itself.

# ── Empirical-Bernstein half-width ────────────────────────────────────
# Total error budget δ, spent across an unbounded number of peeks via a per-m
# union bound: δ_m = δ·(6/π²)/m², so Σ_m δ_m ≤ δ and the interval is valid at
# EVERY block boundary (no optional-stopping inflation — the classic trap of
# "peek at the CI and stop when it looks tight").
(def EB-DELTA 0.000001)
# 1e-6 — two-sided, anytime-valid
(def INV-PI2-6 0.6079271018540267)
# 6/π², the union-bound normalizer

(defn eb-halfwidth [m var rng]
  "Anytime-valid empirical-Bernstein half-width on the per-op-rate mean after m
   block samples with sample variance VAR and observed range RNG. The linear
   term uses the OBSERVED range, so a deterministic leak (rng 0, var 0) has
   half-width 0 and converges at the floor. Maurer–Pontil form; instrument
   only (the soundness oracle is --trace=guardfree)."
  (if (< m 2)
    (math/inf)
    (let [dm (/ (* EB-DELTA INV-PI2-6) (* m m))
          l (math/log (/ 3.0 dm))]
      (+ (math/sqrt (/ (* 2.0 (* var l)) m)) (/ (* 3.0 (* rng l)) m)))))

# ── The gauges ────────────────────────────────────────────────────────
# One instrument: the dimension it reads, the unit one point of a rate on it
# carries, and the reading itself. The axis rides the GAUGE rather than the
# probe's label because that is where it is a fact — every probe already hands
# the estimator the gauge it wants, so no probe declares an axis and none can
# declare the wrong one (docs/test-store.md § Measurements).
#
# `read` is pulled out once, before a measurement starts, and called inside the
# window exactly as a bare gauge function was: a struct read between the two
# samples would be work the gauge is supposed to be measuring.
(defn gauge [axis unit read]
  {:axis axis :unit unit :read read})

(def count-gauge (gauge "objects" "objects/op" (fn [] (arena/count))))
# object-count gauge
(def bytes-gauge (gauge "bytes" "bytes/op" (fn [] (arena/bytes))))
# bump-arena bytes gauge
(def region-gauge (gauge "regions" "regions/op" (fn [] (arena/region-count))))
# live-region-entry gauge — every active RegionEntry counts, a pages-less owner
# node included. What the object count cannot see is a ZERO-OBJECT entry: an
# owner node (docs/impl/region/owner.md § "Owner nodes"), or a region emptied
# of objects but pinned by an unbalanced count.
(def ids-gauge (gauge "ids" "ids/op" (fn [] (arena/region-ids))))
# physical-id issuance gauge — `next_physical`, the dimension every other gauge
# is blind to (docs/impl/region/diagnostics.md § the `arena/region-ids` bullet).
# A physical id minted for a call whose callee allocates nothing never becomes a
# live region, so it holds no object, no page, no bytes and no reference count,
# and `count-gauge`/`bytes-gauge`/`region-gauge` all read flat while it strands.
# A mint that finds an id on the free list leaves `next_physical` alone, so a
# steady-state loop holds this gauge flat and every unit of rate is an id that
# did not come back. `arena/region-table` is NOT a second gauge here: it is sized
# by the largest id ever made live from EITHER id source — the per-heap counter
# this gauge reads, and the raw static-slot ids whose range sits far above it —
# so its high-water mark is already past anything a loop driving the counter can
# reach, and it cannot move for any probe shape. An unmovable gauge paints every
# verdict green, which is what the discriminator discipline exists to refuse.

# ── The sequential estimator (general core) ───────────────────────────
# RUN-BLOCK is (fn [b]) that performs b ops on the heap; GAUGE is one of the
# instruments above, which reads the heap measure and names the dimension it
# read. Parameterizing both lets one estimator serve every probe shape: a
# while-loop of thunks, a tail-recursion, a fiber driven by external resumes,
# and a bytes gauge — the per-op rate is (Δgauge)/b per block regardless of HOW
# the b ops ran.
(defn measure-core [label run-block gauge block minb maxb epsilon tau]
  "Adaptive empirical-Bernstein leak-rate estimator. Returns a struct with the
   measured :rate, :half (half-width), :blocks, :ops, and a :verdict:
     :closed       — rate + half < TAU   (reclaimed / bounded)
     :open         — rate - half > TAU   (leaking at ≥ TAU per op)
     :inconclusive — the interval straddles TAU.
   The first block is warmup (discarded — it carries the one-time intercept)."
  # every caller is in another compile unit, so no call-site param join can
  # prove these; the allocation-free diverging guards do.
  (when (%not (%int? block)) (error :block-not-int))
  (when (%not (%int? minb)) (error :minb-not-int))
  (when (%not (%int? maxb)) (error :maxb-not-int))
  (run-block block)  # warmup block, discarded
  # Resolved once, outside every measurement window: the reading is what the
  # window must hold, not the lookup that finds it.
  (def read (get gauge :read))
  (def @m 0)
  (def @mean 0.0)
  (def @m2 0.0)
  (def @lo (math/inf))
  (def @hi (math/-inf))
  (def @half (math/inf))
  (def @blk 0)
  (while (and (%lt blk maxb) (or (%lt blk minb) (not (< half epsilon))))
    (let [before (read)]
      (run-block block)
      # READ is a closure VALUE, so its results are untyped; the diverging
      # %int? guards prove them for the %sub operand contract. Single-opcode
      # predicates, placed after BOTH reads — nothing they do can land inside
      # the [before, after] measurement window.
      (let [after (read)]
        (when (%not (%int? before)) (error :gauge-not-integer))
        (when (%not (%int? after)) (error :gauge-not-integer))
        (let [net (%sub after before)
              x (/ (float net) (float block))]
          (assign m (%add m 1))
          (let [delta (- x mean)]
            (assign mean (+ mean (/ delta m)))
            (assign m2 (+ m2 (* delta (- x mean)))))  # Welford: uses updated mean
          (when (< x lo) (assign lo x))
          (when (> x hi) (assign hi x))
          (assign
            half
            (eb-halfwidth m (if (< m 2) 0.0 (/ m2 (- m 1))) (- hi lo))))))
    (assign blk (%add blk 1)))
  (let [verdict (cond
                  (< (+ mean half) tau) :closed
                  (> (- mean half) tau) :open
                  :inconclusive)]
    {:label label
     :axis (get gauge :axis)
     :unit (get gauge :unit)
     :rate mean
     :half half
     :blocks blk
     :ops (%mul blk block)
     :verdict verdict}))

(defn run-thunk-block [probe b]
  "Run PROBE b times, passing the iteration index — the run-block for direct-loop
   probes. PROBE is (fn [j]): j varies the input so a body cannot constant-fold,
   faithful to the originals' use of the loop variable i."
  # b arrives through a closure value (untyped); the allocation-free diverging
  # guard proves it for the loop's %lt. PROBE is a closure, so a blanket
  # (numeric!) would be wrong here.
  (when (%not (%int? b)) (error :block-not-int))
  (def @j 0)
  (while (%lt j b)
    (probe j)
    (assign j (%add j 1))))

(defn stmt-run [thunk]
  "Run THUNK b times as a discarded STATEMENT (non-tail) — the while-loop shape
   a per-call leak needs to surface; a thunk wrapper's return convention would
   reclaim the discarded-statement over-keep on its own."
  (fn [b]
    (when (%not (%int? b)) (error :block-not-int))
    (def @i 0)
    (while (%lt i b)
      (thunk)
      (assign i (%add i 1)))))

(defn measure [label probe block minb maxb epsilon tau]
  "Direct-loop wrapper: PROBE is a per-op thunk, gauge is the object count."
  (measure-core label (fn [b] (run-thunk-block probe b)) count-gauge block minb
                maxb epsilon tau))

# ── B-invariance: the instrument's second self-test ───────────────────
# A measured rate is a true PER-OP rate only if it is invariant to the block
# size B. If the gauge accumulates a fixed constant per BLOCK (a scheduler pump
# allocating once per batch, say), then net/B = true_rate + C/B, which SHIFTS
# with B — a confidently-wrong number. So a reported leak rate is measured at
# two block sizes (b and 2b) and the intervals must overlap; if they do not,
# the rate is not a per-op rate and the verdict is :contaminated, NOT a number.
# This is the peer of the gauge-live discriminator: that proves the gauge
# moves, this proves the rate means what it claims.
(defn agree? [a b]
  "Do two measurements' rate intervals overlap (a consistent per-op rate)?"
  (not (or (< (+ (get a :rate) (get a :half)) (- (get b :rate) (get b :half)))
           (< (+ (get b :rate) (get b :half)) (- (get a :rate) (get a :half))))))

(defn measure-stable [label probe b minb maxb epsilon tau]
  "Measure PROBE at block sizes b and 2b and cross-check B-invariance. Returns
   the finer (larger-B) measurement, carrying :alt-rate (the rate at b) and a
   :verdict overridden to :contaminated when the two intervals do not overlap."
  (when (%not (%int? b)) (error :block-not-int))
  (let [a (measure label probe b minb maxb epsilon tau)
        c (measure label probe (%mul 2 b) minb maxb epsilon tau)]
    (put (put c :alt-rate (get a :rate))
         :verdict (if (agree? a c) (get c :verdict) :contaminated))))

# ── The two-gauge reading — one drive, one verdict per (probe, gauge) ─
# Both gauges sample around the SAME blocks, so the two verdicts price the same
# ops and the second reading adds two primitive calls per block, never a second
# run of the probe. Both gauges are Immediate, so one gauge's read inside the
# other's [before, after] window moves nothing. Each gauge keeps its own
# estimator state, epsilon, and tau, and blocks continue until BOTH half-widths
# are under their own epsilon (bounded by maxb); each verdict comes from its
# own gauge's interval. δ is spent once per gauge per measurement — at 1e-6 the
# union over a whole dashboard run stays below 2e-4.
(defn measure-2 [label run-block ga ea ta suffix gb eb tb block minb maxb]
  "Drive RUN-BLOCK once per block, sampling gauges GA and GB before and after;
   return [result-a result-b]. Result b is labelled label@SUFFIX — a full label
   of its own everywhere (`by-design`, `declare-root`, the pins), so each
   dimension is independently a closed control or a declared defect."
  (when (%not (%int? block)) (error :block-not-int))
  (when (%not (%int? minb)) (error :minb-not-int))
  (when (%not (%int? maxb)) (error :maxb-not-int))
  (run-block block)  # warmup block, discarded
  # Both readings resolved once, outside every window (see measure-core).
  (def reada (get ga :read))
  (def readb (get gb :read))
  (def @ma 0)
  (def @meana 0.0)
  (def @m2a 0.0)
  (def @loa (math/inf))
  (def @hia (math/-inf))
  (def @halfa (math/inf))
  (def @mb 0)
  (def @meanb 0.0)
  (def @m2b 0.0)
  (def @lob (math/inf))
  (def @hib (math/-inf))
  (def @halfb (math/inf))
  (def @blk 0)
  (while (and (%lt blk maxb)
              (or (%lt blk minb) (not (and (< halfa ea) (< halfb eb)))))
    (let [before-a (reada)
          before-b (readb)]
      (run-block block)
      (let [after-a (reada)
            after-b (readb)]
        # gauge results arrive through closure values (untyped); the diverging
        # %int? guards prove them for %sub, placed after ALL FOUR reads so
        # nothing they do can land inside either measurement window.
        (when (%not (%int? before-a)) (error :gauge-not-integer))
        (when (%not (%int? before-b)) (error :gauge-not-integer))
        (when (%not (%int? after-a)) (error :gauge-not-integer))
        (when (%not (%int? after-b)) (error :gauge-not-integer))
        (let [xa (/ (float (%sub after-a before-a)) (float block))
              xb (/ (float (%sub after-b before-b)) (float block))]
          (assign ma (%add ma 1))
          (let [da (- xa meana)]
            (assign meana (+ meana (/ da ma)))
            (assign m2a (+ m2a (* da (- xa meana)))))
          (when (< xa loa) (assign loa xa))
          (when (> xa hia) (assign hia xa))
          (assign
            halfa
            (eb-halfwidth ma (if (< ma 2) 0.0 (/ m2a (- ma 1))) (- hia loa)))
          (assign mb (%add mb 1))
          (let [db (- xb meanb)]
            (assign meanb (+ meanb (/ db mb)))
            (assign m2b (+ m2b (* db (- xb meanb)))))
          (when (< xb lob) (assign lob xb))
          (when (> xb hib) (assign hib xb))
          (assign
            halfb
            (eb-halfwidth mb (if (< mb 2) 0.0 (/ m2b (- mb 1))) (- hib lob))))))
    (assign blk (%add blk 1)))
  [{:label label
    :axis (get ga :axis)
    :unit (get ga :unit)
    :rate meana
    :half halfa
    :blocks blk
    :ops (%mul blk block)
    :verdict (cond
               (< (+ meana halfa) ta) :closed
               (> (- meana halfa) ta) :open
               :inconclusive)}
   {:label (string label "@" suffix)
    :axis (get gb :axis)
    :unit (get gb :unit)
    :rate meanb
    :half halfb
    :blocks blk
    :ops (%mul blk block)
    :verdict (cond
               (< (+ meanb halfb) tb) :closed
               (> (- meanb halfb) tb) :open
               :inconclusive)}])

# ── The ledger ────────────────────────────────────────────────────────
# The failure-accumulating runner. Each (check …) evaluates its body under
# protect and RECORDS a blown assertion instead of aborting the file, so one
# red probe never masks the rest; (report) at the end re-raises ONE assertion
# naming every failure (non-zero exit).
(def @failures @[])
(defn fail! [msg]
  (push failures msg))
(defmacro check (& body)
  `(let [[ok? v] (protect ,;body)]
     (unless ok?
       (fail! (if (struct? v) (get v :message) (string v))))))
(defn report []
  (assert (= (length failures) 0)
          (string (length failures) " probe(s) failed:\n  "
                  (string/join failures "\n  "))))

# The defect / by-design split. The dashboard populates `by-design` (its fixed
# growth set) and calls `declare-root` for its open probes; `classify` folds
# each measured verdict into the split accumulators, and `stats` hands them
# back for the headline and the completeness gates. A by-design open probe
# DISPLAYS :growth so `grep -c '^  open'` counts defects alone; the measured
# :verdict is untouched, so gates that read it are unaffected.
(def @by-design @{})
(def @root-of @{})
(defn declare-root [root labels]
  (each l in labels
    (put root-of l root)))
(def @n-defects 0)
(def @n-by-design 0)
(def @roots-seen @{})
(def @unclassified @[])
(defn classify [label verdict]
  "Fold one probe's MEASURED verdict into the split accumulators and return its
   DISPLAY verdict. A by-design open probe shows :growth (tallied by-design); a
   classified defect shows :open (tallied, its root recorded); an open probe in
   NEITHER table shows :open and is recorded unclassified — the completeness gate
   fails on it. :closed / :inconclusive pass through untallied."
  (if (not= verdict :open)
    verdict
    (if (get by-design label)
      (begin
        (assign n-by-design (%add n-by-design 1))
        :growth)
      (begin
        (assign n-defects (%add n-defects 1))
        (let [root (get root-of label)]
          (if (nil? root) (push unclassified label) (put roots-seen root true)))
        :open))))

# ── The measurement channel (docs/test-store.md § Measurements) ───────
# Every verdict is reported as well as printed: one JSON object per line,
# appended to the file ELLE_TEST_MEASUREMENTS names. Unset, the channel is
# closed and nothing is written.
#
# Read once, into a binding: the variable cannot change under a running
# process, and a dashboard reports a few hundred times.
(def measurement-sink (sys/env "ELLE_TEST_MEASUREMENTS"))

(defn subject-of [label]
  "A probe's label without the `@axis` suffix `measure-2` gives its second
   reading. The suffix is a rendering — the axis is a column of its own — so
   one probe read on two dimensions stays one subject a query can group by."
  (get (string/split label "@") 0))

(defn report-measurement [r verdict]
  "Append one verdict to the channel, or do nothing when it is closed. Appended
   rather than rewritten, because every probe reports through here and the file
   accumulates a dashboard's whole run. Best-effort under `protect`: a
   dashboard's verdicts are what it exists to produce, and an unwritable sink
   must not turn a measured run into a failed one."
  (when measurement-sink
    (protect (let [p (port/open measurement-sink :append)]
               (port/write p
                           (string (json/serialize {:subject (subject-of (get r
                                   :label))
                                   :axis (get r :axis)
                                   :value (get r :rate)
                                   :unit (get r :unit)
                                   :verdict (string verdict)}) "\n"))
               (port/close p))))
  nil)

(defn show [r]
  "Print one measured class as a dashboard line, and report it through the
   channel. `classify` runs once for both: it tallies as it goes, so a second
   call would count the same probe twice."
  (let [alt (get r :alt-rate)
        verdict (classify (get r :label) (get r :verdict))]
    (report-measurement r verdict)
    (println "  " verdict "  " (get r :label) ": rate=" (get r :rate) " ±"
             (get r :half) (if (nil? alt) "" (string " [B-check " alt "]"))
             "  (" (get r :ops) " ops / " (get r :blocks) " blocks)")))

# A pinned rate is an exact number (matched within ±0.5 — integer resolution on
# the real-valued estimate) or a [lo hi] inclusive range (for the rare shape
# whose true rate genuinely spans across tiers).
(defn match-rate? [got want]
  (if (array? want)
    (and (not (< got (get want 0))) (not (< (get want 1) got)))
    (and (< (- got want) 0.5) (< (- want got) 0.5))))

(defn pin [r want]
  "The single assertion shape for every class — table-driven or bespoke.
   Shrink-only: a fix LOWERS the pin."
  (show r)
  (let [[ok? v] (protect (assert (match-rate? (get r :rate) want)
                                 (string (get r :label) ": pinned " want
                                 ", measured " (get r :rate) " ("
                                 (get r :verdict) ") — shrink-only")))]
    (unless ok?
      (fail! (if (struct? v) (get v :message) (string v))))))

(defn stats []
  "The split accumulators, read at a run's end for the headline and gates."
  {:defects n-defects
   :by-design n-by-design
   :roots (length (keys roots-seen))
   :unclassified unclassified})
