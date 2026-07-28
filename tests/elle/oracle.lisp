(elle/epoch 12)
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
# The failure-accumulating runner. Each (check …) evaluates its body under
# protect and RECORDS a blown assertion instead of aborting the file, so one red
# probe never masks the rest; (report) at the end re-raises ONE assertion naming
# every failure (non-zero exit).
(def @failures @[])
(defmacro check (& body)
  `(let [[ok? v] (protect ,;body)]
     (unless ok?
       (push failures (if (struct? v) (get v :message) (string v))))))
(defn report []
  (assert (= (length failures) 0)
          (string (length failures) " probe(s) failed:\n  "
                  (string/join failures "\n  "))))

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

# ── The sequential estimator (general core) ───────────────────────────
# RUN-BLOCK is (fn [b]) that performs b ops on the heap; GAUGE is (fn []) that
# returns the heap measure (object count or bytes). Parameterizing both lets one
# estimator serve every probe shape: a while-loop of thunks, a tail-recursion, a
# fiber driven by external resumes, and a bytes gauge — the per-op rate is
# (Δgauge)/b per block regardless of HOW the b ops ran.
(defn measure-core [label run-block gauge block minb maxb epsilon tau]
  "Adaptive empirical-Bernstein leak-rate estimator. Returns a struct with the
   measured :rate, :half (half-width), :blocks, :ops, and a :verdict:
     :closed       — rate + half < TAU   (reclaimed / bounded)
     :open         — rate - half > TAU   (leaking at ≥ TAU per op)
     :inconclusive — the interval straddles TAU.
   The first block is warmup (discarded — it carries the one-time intercept)."
  (run-block block)  # warmup block, discarded
  (def @m 0)
  (def @mean 0.0)
  (def @m2 0.0)
  (def @lo (math/inf))
  (def @hi (math/-inf))
  (def @half (math/inf))
  (def @blk 0)
  (while (and (%lt blk maxb) (or (%lt blk minb) (not (< half epsilon))))
    (let [before (gauge)]
      (run-block block)
      # GAUGE is a closure VALUE, so its results are untyped; the diverging
      # %int? guards prove them for the %sub operand contract. Single-opcode
      # predicates, placed after BOTH reads — nothing they do can land inside
      # the [before, after] measurement window.
      (let [after (gauge)]
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

(defn count-gauge []
  (arena/count))
# object-count gauge
(defn bytes-gauge []
  (arena/bytes))
# bump-arena bytes gauge

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
  (let [a (measure label probe b minb maxb epsilon tau)
        c (measure label probe (%mul 2 b) minb maxb epsilon tau)]
    (put (put c :alt-rate (get a :rate))
         :verdict (if (agree? a c) (get c :verdict) :contaminated))))

# ── The defect / by-design split — the instrument owns the burndown headline ──
# Every leak class has a ROOT (F1a/F1b/F2/F3/F4/F5), declared below. A small fixed set
# of probes read open BY DESIGN — the module-level live-growth discriminator, the
# sub-integer estimator self-test, and `push-accum` (whose residual is genuine per-op
# `map` scratch it retains, § F1a) — and are NOT counted as defects. The open/closed
# split and the defect-vs-by-design breakdown used to be
# recovered from this dashboard by `grep -c` minus a hand count of them; the
# classifier below prints it directly AND refuses to be silently wrong: every
# probe that MEASURES :open must be declared here (a root, or by-design), or the
# completeness gate at the end fails. A by-design open probe DISPLAYS :growth so
# `grep -c '^  open'` counts defects alone; the measured :verdict is untouched, so
# the gauge-live and B-invariance gates (which read it) are unchanged.
# `push-outer` and `recur-local-foreign-mint` are NOT here: their apparent growth
# was the F1b push-container over-keep of a BLOCK-LOCAL accumulator (freed at the
# block's return once the wrapper stops stranding its owned-param reference), not
# genuine unbounded retention — the real gauge-live discriminator (`probe-disc`)
# uses a MODULE-LEVEL sink and is unaffected. They are now CLOSED controls
# (undeclared, like `rest-array-copy`), so a regression to open trips the
# completeness gate as an F1b defect rather than being absorbed as growth.
(def @by-design
  @{"discriminator (live-growth)" true
    "sub-integer (1-in-3 retain)" true
    "push-accum" true})
(def @root-of @{})
(defn declare-root [root labels]
  (each l in labels
    (put root-of l root)))
# `rest-array-copy` is a CLOSED control (the native fresh-result invariant, not F1a
# stdlib-body scratch) — undeclared like `slice`/`to-array`, so a regression to open
# trips the completeness gate loudly rather than being silently absorbed as F1a.
# `map-while`/`filter-while` are undeclared for the same reason: a fusable kernel
# dissolves, so they are CLOSED dissolution controls and a regression to open must
# trip the gate loudly instead of being absorbed as F1a scratch that is no longer
# there. The un-fused op's scratch keeps its F1a declaration through `wrap-map`.
(declare-root :f1a ["reduce" "fold" "stdlib-fold" "wrap-map" "distinct"
                    "group-by" "frequencies" "merge" "concat" "pipeline"
                    "each-list" "string-outer" "append-outer" "concat-while"
                    "yield-concat" "nested-closure" "stdlib-concat" "zip-tower"])
(declare-root :f1b ["mut-array-push" "mut-string" "struct-put" "push-churn"
                    "put-churn" "store-wrapper" "native-tail-put-struct"
                    "native-tail-put-array" "native-tail-del-ctl" "pop-wrapper"
                    "del-wrapper" "set-del-wrapper" "set-add"])
(declare-root :f2 ["fiber-nested" "multi-resume" "yield-discard"
                   "yield-multimut" "protect-while" "denied-discard"
                   "cancel-discard" "abort-discard"])
(declare-root :f3 ["io-yield ev/sleep"])
# `recur-local-self-mint` is F4 (cyclic refusal-to-Shared): a self-recursive local
# closure that RETURNS ITSELF is a genuine reference cycle RC cannot collect, so it
# stays Shared when its holder frees. Its acyclic control `recur-local-foreign-mint`
# (captures an immediate, not itself) now reclaims to 0, isolating the residual to
# the self-reference cycle — the "mint-count" gap the pair was built to expose,
# unmasked once the F1b container over-keep on their shared block-local `@keep`
# accumulator closed.
(declare-root :f4 ["recur-local-self-mint"])
# The whole `break-*` family is CLOSED controls (undeclared, like
# `rest-array-copy`), so a regression to open trips the completeness gate loudly
# instead of being absorbed as F5: `break-value*` pin the break TRANSFER (the
# value the break carries dies where the block's value dies) and `break-skipped`
# pins the window the jump passes over (every OTHER release between the break
# site and the exit label is re-anchored to the block).
# `take`/`drop`/`zip` are CLOSED controls for the per-path return frontier
# (undeclared, like `rest-array-copy`): all three are `letrec` walks whose base case
# returns a heap value the recursive arm's `decref_point` was left to release, so a
# regression must trip the completeness gate rather than be absorbed as an F5 strand.
# `match-dead-arm` and `match-used-arm` are CLOSED controls for the two faces of a
# region live-in to a branch (undeclared, like `rest-array-copy`): the dead arm
# takes per-arm compensation, and the USED arm is covered by anchoring the
# region's one release where every arm reaches it — the branch-arm release window,
# whose owned-parameter and `If` faces are `param-used-arm`/`param-used-arm-if`.
# `struct-outer` and `yield-reassign` are CLOSED controls for the fn-local 1-slot
# container (undeclared, like `rest-array-copy`), and they must stay a PAIR: both
# drive the container's two release channels — drop-on-overwrite for each displaced
# prior, the content drop at the cell's demise for the final one, with the
# producer's separate claim released at the store — but only `yield-reassign` has a
# HEAP init, the shape that reaches the gate's sole-held check, where
# functionalization's split of the cell's source name into a pre-loop version and a
# loop parameter reads as two holders of one name. A regression of either must trip
# the completeness gate rather than hide behind its sibling.
(declare-root :f5 ["raw-del" "raw-del-immediate" "fresh-env-cell" "struct-match"])

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

(defn show [r]
  "Print one measured class as a dashboard line."
  (let [alt (get r :alt-rate)]
    (println "  " (classify (get r :label) (get r :verdict)) "  " (get r :label)
             ": rate=" (get r :rate) " ±" (get r :half)
             (if (nil? alt) "" (string " [B-check " alt "]")) "  (" (get r :ops)
             " ops / " (get r :blocks) " blocks)")))

# ── Probes ────────────────────────────────────────────────────────────

# Live-growth discriminator: a genuine unbounded retain. Every op pushes a
# fresh struct into a module-level @array, which keeps it forever, so the gauge
# MUST climb ~1/op. If this does not read :open the gauge is dead.
(def @disc-sink @[])
(defn probe-disc [j]
  (push disc-sink {:k j}))

# Bounded shape: an immutable struct built and immediately dropped — the
# reclaimed baseline (the leak suite pins this at slope 0). Should read :closed.
(defn probe-bounded [j]
  {:x j :y 2})

# The io-yield target: ev/sleep, the clean probe (portless, nil result). Pins the
# per-op net-object residual of a yielding io op. The residual is NOT an io-local
# mechanism: it is a general escape-imprecision leak the scheduler pump hits each
# op — the completion struct held Shared because escape analysis cannot see
# ev/sleep's resume value is nil, so `(get c :value)` flowing into `fiber/resume`
# marks the struct escaping; this shrinks only as escape analysis is made
# branch/path-sensitive. (The IN-LAMBDA self-recursive letrec closure `+`/`<` build over
# their varargs is cell-free — its self-reference resolves to the executing closure, no
# cell↔closure cycle — reclaimed per call by ordinary RC / the tail-call deferred release,
# docs/impl/selfrec.md; that class is pinned directly by the `recur-local-self` probe
# below.)
(defn probe-io-yield [j]
  (ev/sleep 0))

# Sub-integer leak: leaks one object every 3 ops = 0.333/op. The OLD integer
# slope floors this to 0 ("reclaimed") — a real unbounded leak made invisible.
# This estimator catches it: measured with a tight tau it reads :open at ≈0.33.
(def @third-sink @[])
(def @third-ctr 0)
(defn probe-third [j]
  (assign third-ctr (%add third-ctr 1))
  (when (%lt 2 third-ctr)  # ctr reached 3
    (assign third-ctr 0)
    (push third-sink {:k 1})))

# ── Run ───────────────────────────────────────────────────────────────
(println "── leak oracle ──")

# 1. Gauge-live gate FIRST. Everything downstream is void if this is not :open.
(def disc (measure "discriminator (live-growth)" probe-disc 200 6 60 0.4 0.5))
(show disc)
(check (assert (= (get disc :verdict) :open)
               (string "GAUGE DEAD: discriminator read " (get disc :verdict)
                       " — every 'closed' verdict this run is void")))

# 2. Bounded baseline — must reclaim.
(def bnd
  (measure "bounded (immutable struct, dropped)" probe-bounded 200 6 60 0.4 0.5))
(show bnd)
(check (assert (= (get bnd :verdict) :closed)
               (string "bounded shape leaked: " (get bnd :verdict) " rate="
                       (get bnd :rate))))

# 3. The io-yield retain. Shrink-only: GREEN at the current rate, RED the moment
#    it moves; a fix only LOWERS it (the residual's anatomy is at probe-io-yield).
#    Dropped 2→1 when the scheduler pump's own completion-struct `put` began to
#    monomorphize (the `& rest` wrapper collection fix, `monomorphize.rs`): one of
#    the two retains was a `put`-wrapper strand, not escape imprecision. The
#    residual 1/op is the genuine F3 Shared-completion retain, closing only as
#    escape analysis is made branch/path-sensitive.
(def io (measure-stable "io-yield ev/sleep" probe-io-yield 200 8 80 0.4 0.5))
(show io)
(check (assert (not= (get io :verdict) :contaminated)
               (string "io-yield rate is block-dependent (B vs 2B): "
                       (get io :rate) " vs " (get io :alt-rate)
                       " — a per-block artifact, not a per-op rate")))
(check (assert (= (get io :verdict) :open)
               (string "io-yield not measured as leaking: " (get io :verdict))))
(check (assert (and (< 0.5 (get io :rate)) (< (get io :rate) 1.5))
               (string "io-yield rate " (get io :rate)
                       " ∉ [0.5,1.5] — shrink-only")))

# 4. Sub-integer leak the integer slope cannot see. Tight tau/epsilon; reads
#    :open at ≈0.33 where `slope` reported 0.
(def sub (measure "sub-integer (1-in-3 retain)" probe-third 300 8 200 0.05 0.1))
(show sub)
(check (assert (= (get sub :verdict) :open)
               (string "sub-integer leak floored to " (get sub :verdict)
                       " rate=" (get sub :rate)
                       " — the estimator must catch what "
                       "integer slope cannot")))
(check (assert (and (< 0.28 (get sub :rate)) (< (get sub :rate) 0.40))
               (string "sub-integer rate " (get sub :rate)
                       " ∉ [0.28,0.40] (expect 0.33)")))

# ── The folded leak suite ─────────────────────────────────────────────
# One dashboard covering every leak class (each declared a root below), on the estimator. The
# shapes need different DRIVERS — one run-block per shape, all feeding the one
# measure-core:
#   - direct-loop (the table below): a per-op thunk run b times;
#   - tail-call rotation: the recursive call itself is the run-block;
#   - fiber-internal yield: a fiber that runs b iterations then completes, drained
#     (a drained loop reclaims at scope-exit; a forever-generator never exits its
#     loop, so its per-iteration values would falsely read as leaks);
#   - persistent containers: the container is def'd fn-local in the run-block;
#   - discarded call-result / break-escape / match-scrutinee: a DIRECT
#     while-statement run-block (a thunk's return convention would reclaim the
#     over-keep the discarded-statement shape leaks);
#   - byte-gauge: the same drivers under arena/bytes;
#   - value-survival: plain asserts (correctness, not a rate).
#
# Each pin is the TRUE CURRENT rate the estimator measures, exact (or a
# [lo hi] range) and shrink-only: a fix LOWERS it, never raises it.

(defn make-struct [i]
  # `i` reaches the value position (:iter i), which disables call-site param
  # joins, so the %add operand is proven by a local coerce-guard instead
  # (docs/intrinsics.md § The contract). The coerce rebinds i to an int without a
  # branch-compensation retain, so the success path stays at 0/op.
  (let [i (if (%int? i) i 0)]
    {:iter i :val (%add i 1)}))
(defn make-label [i]
  (string "item-" i))
(defn t19-store [c v]
  (put c :x v))
(defn t20-make-cell []
  (def @c 0)
  (let [f (fn []
            (assign c (%add c 1))
            c)]
    (f)))
# The two faces of per-arm compensation over a `Match`, driven through the arm the
# caller picks. `v` is allocated before the dispatch, so it is live-in on every arm
# and its lone `decref_point` lands in the arm that uses it last.
#   DEAD arm  — the taken arm has no use of `v` at all, so it creates no reference
#               and takes the head release (`regions::compensate`).
#   USED arm  — the taken arm uses `v` but is not the one holding the `decref_point`,
#               and no retain on its last-use node funds a per-arm release, so it
#               keeps the conservative baseline and strands `v` (F5).
(defn t21-dead-arm [t]
  (let [v (list 1 2 3)]
    (match t
      :use (length v)
      :skip 0
      _ -1)))
(defn t21-used-arm [t]
  (let [v (list 1 2 3)]
    (match t
      :a (length v)
      :b (length v)
      _ (length v))))
# The same arm structure over an OWNED PARAMETER rather than a fn-local — the
# polymorphic stdlib entry point's shape, whose caller moved the argument in. The
# region's one release is anchored where every arm reaches it, so the arm the
# caller happens to pick does not decide whether the argument is freed
# (docs/impl/region/mechanism.md § "A release inside one arm is not a release on
# the other arms"). `t22-param-if` is the `If` face of the identical premise:
# the window reads arm structure, never the branch's kind or arity.
(defn t22-param-arm [v t]
  (match t
    :a (length v)
    :b (length v)
    _ (length v)))
(defn t22-param-if [v c]
  (if c (length v) (%add 1 (length v))))
# The frame-exit release (docs/impl/region/mechanism.md § "A release past a
# frame-replacing tail call is not a release"). `t23-unused`'s parameter is used
# nowhere, so its release is the unused-parameter fallback the lowerer emits at the
# end of the body — the block a CLOSURE callee never reaches — and escape clears it
# as sole-held. `t23-moved` is the exemption: its parameter IS the tail call's
# argument, so its release is the ownership move and must stay put.
(defn t23-sink []
  0)
(defn t23-unused [x]
  (t23-sink))
(defn t23-take [a]
  (length a))
(defn t23-moved [x]
  (t23-take x))
# `t23-arms` puts the same stranded release past a MERGE: both arms leave through
# a frame-replacing tail call, so the block the release lands in is reached on
# neither path. A branch merge inherits its arms' relocation points, so the
# release is replicated ahead of each arm's `TailCall` as well as emitted at the
# merge — sound because a value-routed release nil-stamps the slot it read, so
# the copy a path reaches second no-ops.
(defn t23-sink2 []
  1)
(defn t23-arms [x t]
  (if t (t23-sink) (t23-sink2)))
(defn helper-f [x]
  (string "v" x))
(defn helper-g [x]
  {:val x})
(defn helper-h [x]
  (+ x 1))
# `op` (a heap arg) is consumed only on the cold error path; on the success path
# its release would land in a branch the path never takes. Pins the cross-function
# per-path branch-compensation case (the check-comparable shape).
(defn check-arg [op a]
  (when (%not (number? a)) (string op " bad"))
  a)
(defn process [i]
  # called only through probe closures, so i is otherwise untyped
  (when (%not (%int? i)) (error :i-not-int))
  (make-struct (%add i 10)))
(defn t17-h []
  {:a 1})
(defn t17-h2 []
  {:b 2})
(defn cyc-mk []
  "A returned a<->b cycle — the transferred-returned-subtree shape (the
   %array-push stores keep the containment visible at this site in both
   intrinsics modes)."
  (let [a @[]
        b @[]]
    (%array-push a b)
    (%array-push b a)
    a))
(defn make-module []
  (defn mod-make [i]
    {:x i})
  (defn mod-label [i]
    (string "item-" i))
  {:make mod-make :label mod-label})
(defn make-heap-module []
  (defn do-process [i]
    {:x i :label (string "item-" i)})
  {:process do-process})
(def @t13proc (fn [i] {:x i}))
(def @t13cond (if (= :fast :fast) (fn [x] {:fast x}) (fn [x] {:slow x})))
(def @t13nested (fn [x] {:x x}))
(def the-mod (make-module))
(def heap-mod (make-heap-module))
(def @t19s @{:x 0})
(def @t20c 0)

# Direct-loop class. Each entry: [label (fn [j] body) rate].
# j varies the input (faithful to the originals' loop variable i). Pins are the
# TRUE CURRENT rate the estimator measures — cross-validated against the source
# files' own slope, several of which are stale (the files are RED there).
(def suite-classes
  [# scope reclamation
   ["discard-struct" (fn [j] {:x j :y (+ j 1)}) 0]
   ["string-alloc" (fn [j] (string "iter-" j)) 0]
   ["pair" (fn [j] (pair j (list))) 0]
   ["let-struct"
    (fn [j]
      (let [x {:iter j}]
        x)) 0]
   ["traited"
    (fn [j]
      (let [t (with-traits @[1 2 3] {:tag :x})]
        (get (traits t) :tag))) 0]
   ["closure-template"
    (fn [j]
      (when (%not (%int? j)) (error :j))
      (let [f (fn [x] (%add x j))]
        (f 1))) 0]  # Per-path branch compensation (src/hir/regions/compensate.rs). A value
   # live-in to a branch but used in only ONE arm is freed on the used path by its
   # in-arm decref AND on every other path by a compensating release at the dead
   # arm's head, so it reclaims on every path — not only the one reaching its last
   # use. Without it, a value whose sole use sits in a never-taken arm leaks 1/op.
   # Each probe forces the NO-USE arm to be the taken one (its `if` cond always
   # falls to the arm that does not reference the value), so the pre-fix rate is 1.
   ["branch-one-arm"
    (fn [j]
      (let [op (string "v" j)]
        (if (number? j) (%gt j 0) (string? op)))) 0]
   ["branch-fresh-arm"
    (fn [j]
      (let [s {:k j}]
        (if (number? j) (%gt j 0) (get s :k)))) 0]
   ["branch-nested"
    (fn [j]
      (let [op (string "v" j)]
        (if (number? j) (if (%lt j 999999) 1 (string? op)) 9))) 0]
   ["branch-error-arg" (fn [j] (check-arg (string "op" j) j)) 0]  # Comparison builtins reclaim: `check-comparable`'s op-name is an interned
   # keyword (no per-call alloc), and were it a heap string the compensation above
   # would still reclaim it — its uses sit only in the cold error arms.
   ["cmp-gt" (fn [j] (> j 0)) 0] ["cmp-lt" (fn [j] (< j 0)) 0]
   ["cmp-ge" (fn [j] (>= j 0)) 0]
   ["fiber-drop"
    (fn [j]
      (let [f (fiber/new (fn [] 7) 2)]
        f)) 0]
   ["fiber-resume"
    (fn [j]
      (let [f (fiber/new (fn [] 7) 2)]
        (fiber/resume f))) 0] ["array-literal" (fn [j] [j (+ j 1) (+ j 2)]) 0]
   ["mut-array" (fn [j] @[]) 0]
   ["mut-array-push"
    (fn [j]
      (let [a @[]]
        (push a j)
        a)) 0] ["mut-struct" (fn [j] @{:x j}) 0]
   ## The stdlib `push` wrapper's `:@string` arm reclaims (rate 0). Two strands close:
   ## the `-mut` CONTAINER via `%string-push-mut` (a `MutableString` pass-through — per-arm
   ## container release + tail-retain suppression, like the `@array`/`@struct`/`@set` arms),
   ## and the byte-copy pushed-VALUE (`@string` copies the value's bytes rather than
   ## retaining its region, so `val` strands across the wrapper's arms; the compensation
   ## releases it per-arm from `funnel_bytecopy_value_sites`, sound because the byte-copy
   ## touched neither `val`'s incref nor its decref).
   ["mut-string"
    (fn [j]
      (let [s @""]
        (push s "x")
        s)) 0]
   ["nested-loop"
    (fn [j]
      (def @k 0)
      (while (%lt k 10)
        {:x j :y k}
        (assign k (%add k 1)))) 0]  # collection ops
    ["reduce" (fn [j] (reduce + 0 [1 2 3])) 0]
   ["fold" (fn [j] (fold (fn [a x] (+ a x)) 0 [1 2 3])) 0]
   # zip's F1a copy-scratch is dissolved: the `tuple-at`/output closures that
   # CAPTURED the mutable `arrs`/`out` (a stored closure over a mutable container
   # strands its region) are now cell-free top-level drivers threading them as
   # params (`zip-tuple-at`/`zip-build-array`/`zip-build-list`). What remained was
   # the per-path return frontier (docs/impl/region/mechanism.md § "The return
   # frontier is per-path"): every one of those drivers is a walk whose base case
   # returns a heap argument while the recursive arm holds the `decref_point`, so
   # each call stranded the argument it handed back. A CLOSED control now,
   # undeclared like `rest-array-copy` so a regression trips the completeness gate.
   ["zip" (fn [j] (zip [1 2] [3 4])) 0] ["sort" (fn [j] (sort [3 1 2])) 0]
   # `reverse` is a CLOSED control for the branch-arm release window
   # (docs/impl/region/mechanism.md § "A release inside one arm is not a release
   # on the other arms"): its accumulator is named by every arm of the trailing
   # `(match t :array (freeze r) … _ r)`, so the one release landed in the last
   # arm and every earlier one stranded the whole accumulator. Undeclared, like
   # `rest-array-copy`, so a regression trips the completeness gate loudly rather
   # than being absorbed as F1a transform-scratch.
   ["reverse" (fn [j] (reverse [1 2 3])) 0]  # `(rest array)` copies the tail into a fresh immutable array; its call-result
   # region reclaims on discard (rate 0). The trait-dispatched `Sequence:rest`
   # native allocates the slice into the outer `rest` call's OWN region (the
   # `dispatch_native_call` fresh-result invariant — a fresh native result lives
   # in the call's `alloc_region`, so the consumer's `DecrefValueRegion` frees
   # it), where minting a separate boundary region stranded it. A CLOSED control
   # beside `slice`/`to-array`, shrink-only: RED if a boundary region strands the
   # slice again (runtime::tests::ownership::
   # region_native_trait_dispatch_fresh_result_reclaims). `(rest list)` shares its
   # tail (also 0).
   ["rest-array-copy" (fn [j] (rest [1 2 3 4 5])) 0]
   ["distinct" (fn [j] (distinct [1 2 1 3])) 2]
   # `take`/`drop` are CLOSED controls for the PER-PATH return frontier
   # (docs/impl/region/mechanism.md § "The return frontier is per-path";
   # tests/elle/region-return-arm-escape-leak.lisp). Both are `letrec` walks whose
   # base case returns a heap value while the recursive arm holds its
   # `decref_point`, so the returning arm carried a return mint and no release and
   # each call stranded what it handed back — `drop` its whole input list even at
   # n=0, `take` its reverse-scratch. Undeclared, like `rest-array-copy`: a
   # regression to open must trip the completeness gate loudly.
   ["take" (fn [j] (take 2 (list 1 2 3))) 0]
   ["drop" (fn [j] (drop 1 (list 1 2 3))) 0]
   ["group-by" (fn [j] (group-by odd? [1 2 3 4])) 4]
   ["frequencies" (fn [j] (frequencies [1 2 1 3])) 2]
   ["to-array" (fn [j] (->array (list 1 2 3))) 0]
   ["to-list" (fn [j] (->list [1 2 3])) 0]
   ["freeze" (fn [j] (freeze @[1 2 3])) 0]
   ["slice" (fn [j] (slice [1 2 3 4] 1 3)) 0]  # trailing nil keeps the body's value a discarded STATEMENT, matching the
   # original while-loop (where the alloc is never the loop's tail value)
   ["keys-values"
    (fn [j]
      (keys {:a 1 :b 2})
      (values {:a 1 :b 2})
      nil) 0] ["merge" (fn [j] (merge {:a 1} {:b 2})) 3]
   ["struct-lit" (fn [j] {:x j :y (+ j 1)}) 0]
   ["struct-get"
    (fn [j]
      (let [s {:x j}]
        s:x)) 0]
   ["struct-put"
    (fn [j]
      (let [s @{:x 0}]
        (put s :x j))) 0]
   ["push-churn"
    (fn [j]
      (let [items @[]]
        (push items {:k j}))) 0]  # The capture-back-edge cycle: a container captured by a closure it holds
   # (`m ⊇ c` store, `c ⊇ m` capture). Per-region RC cannot collect the m↔c
   # cycle, and no region root can own it (the captured member's live decref
   # over-extends past the closure), so it leaks per op. The activation-owner cut
   # reclaims the INTRINSIC form of this shape (runtime::tests::ownership::
   # region_ownership_capture_back_edge_cycle_reclaims, without_stdlib /
   # %array-push). CLOSED for the full-stdlib form too: the `(push m c)` that
   # records the `m` contains `c` edge now monomorphizes to `%push-array-mut`
   # cross-unit (mutable @array is a self-reclaiming op, `monomorphize.rs`), so the
   # containment reaches the cut exactly as the intrinsic form does — no surviving
   # wrapper to hide it. A collateral close of the store-family monomorphization.
   ["capture-backedge"
    (fn [j]
      (let [root @[]
            m @[]]
        (let [c (fn [] (length m))]
          (push m c)
          (c)
          (push root m)
          nil))) 0]  # The transferred returned cycle: a helper builds an a<->b cycle and hands
   # its root back across the return frontier; the consumer discards it.
   # Per-region RC cannot collect the cycle (the interior back-edge outlives
   # every release) and no region root can own it (the root crosses the
   # frontier). The transfer cut (owner = the consuming activation's node)
   # reclaims it — rate 0
   # (runtime::tests::ownership::region_ownership_reclaims_returned_cycle_across_calls
   # pins it bounded).
   ["returned-cycle"
    (fn [j]
      (begin
        (cyc-mk)
        nil)) 0]  # string ops + realistic patterns
    ["string-interp" (fn [j] (string "x=" j " y=" (+ j 1))) 0]
   ["concat" (fn [j] (concat "a" "b" "c")) 4]
   ["split" (fn [j] (string/split "a,b,c" ",")) 0]
   ["join" (fn [j] (string/join ["a" "b" "c"] ",")) 0]
   ["trim" (fn [j] (string/trim "  x  ")) 0]
   ["replace" (fn [j] (string/replace "hello" "l" "r")) 0]
   ["num-to-str" (fn [j] (number->string j)) 0] ["read" (fn [j] (read "42")) 0]
   ["call-chain" (fn [j] (helper-f (helper-g (helper-h j)))) 0]  # A fresh heap
   # value (`helper-g` result) stored into a cons via `(pair … …)` — a CLOSED
   # control for the cons-store containment accounting. The `%pair`/`list` opcode
   # (`handle_list`) once increfed each cross-region member by hand AND let the
   # alloc funnel (`alloc_in_region` → `incref_cross_region_refs`) incref+record it
   # again, so each stored heap element was double-counted against the single
   # free-time cascade decref — 1/op per heap member. Now the alloc funnel is the
   # sole containment incref, exactly as `args_to_list` and every native
   # list/array constructor do it (`vm/data.rs handle_list`). Soundness pinned by
   # region-pair-heap-content-uaf.lisp; shrink-only.
   ["arg-result" (fn [j] (pair j (helper-g j))) 0]
   ["let-chain"
    (fn [j]
      (let [a (helper-h j)]
        (let [b (helper-g a)]
          b))) 0]  # `each` over a statically-typed collection reclaims: the literal array's
   # `(match (type-of seq) …)` off-array arms are pruned (typeinfer/prune.rs), so
   # seq lives only in the live arm. `each-manual` is the equivalent indexed loop.
   ["each-array"
    (fn [j]
      (each x in [1 2 3]
        x)) 0]
   ["each-manual"
    (fn [j]
      (let [a [1 2 3]]
        (def @k 0)
        (while (%lt k 3)
          (get a k)
          (assign k (%add k 1))))) 0]
   ["format" (fn [j] (string "iter " j " of " 100)) 0]
   ["pipeline"
    (fn [j]
      (string/join (filter (fn [x] (not= x ""))
                           (map string/trim (string/split "a , b , c" ","))) ","))
    10]
   ["each-list"
    (fn [j]
      (each x in (list 1 2 3)
        {:val x})) 3]
   # `map-while`/`filter-while` are DISSOLUTION controls: a non-capturing kernel
   # over a proven immutable array fuses to an inlined index-walk loop
   # (docs/impl/dissolution.md), so the stdlib op — and every per-call strand it
   # carried (the closure `map` mints for `f`, the `freeze` copy, the map-body
   # over-keep) — ceases to exist and the rate is 0. The residual F1a scratch of
   # the UN-fused op is gauged by `wrap-map` below, whose lambda captures.
   ["map-while"
    (fn [j]
      (map (fn [x]
             (numeric!)
             (%add x 1)) [1 2 3])) 0]
   ["filter-while"
    (fn [j]
      (filter (fn [x]
                (numeric!)
                (%gt x 1)) [1 2 3])) 0]
   ["nested-closure"
    (fn [j]
      (let [f (fn [] (fn [] j))]
        ((f)))) 2]  # user-fn calls, value flow
    ["user-struct" (fn [j] (make-struct j)) 0]
   ["user-string" (fn [j] (make-label j)) 0] ["chain" (fn [j] (process j)) 0]
   # The F1a gauge for the UN-fused stdlib `map`: the kernel CAPTURES `k`, a shape
   # loop fusion declines (splicing a capture at the call site is out of scope), so
   # the real `map` runs and its per-call strands are measured — the closure `map`
   # mints for `f` (F5 arg/closure-retain), the `freeze` copy, and the map-body
   # over-keep. Rate flat in element count: per-call strands, not per-element copy.
   ["wrap-map"
    (fn [j]
      (let [k 1]
        (map (fn [x]
               (numeric!)
               (%add x k)) [1 2 3]))) 3] ["factory" (fn [j] (t13proc j)) 0]
   ["cond-factory" (fn [j] (t13cond j)) 0] ["alias" (fn [j] (make-struct j)) 0]
   ["nested-factory" (fn [j] (t13nested j)) 0]
   ["struct-field"
    (fn [j]
      (the-mod:make j)
      (the-mod:label j)) 0]
   ["heap-struct-field" (fn [j] (heap-mod:process j)) 0]
   ["g-variant"
    (fn [j]
      (let [g (fn [] (%pair j j))]
        (g))) 0]
   ["bound-callee"
    (fn [j]
      (let [f t17-h]
        (f))) 0]
   ["break-skip"
    (fn [j]
      (block (let [a {:k j}]
               (let [b {:a j}]
                 (break))))) 0]
   ["store-wrapper" (fn [j] (t19-store t19s (string "v" j))) 0]
   ["fresh-env-cell" (fn [j] (t20-make-cell)) 1]
   ["shared-env-cell"
    (fn [j]
      (let [f (fn []
                (assign t20c (%add t20c 1))
                t20c)]
        (f))) 0]  # non-yielding fiber / closure / protect loops
   ["closure-while"
    (fn [j]
      (let [f (fn [] j)]
        (f))) 0]
   ["fiber-while"
    (fn [j]
      (let [f (fiber/new (fn [] j) 1)]
        (fiber/resume f))) 0]
   ["concat-while" (fn [j] (concat "x" (number->string j))) 3]
   ["protect-while"
    (fn [j]
      (let [[ok v] (protect ((fn [] j)))]
        v)) 0]
   ["one-shot"
    (fn [j]
      (let [f (fiber/new (fn [] j) 1)]
        (fiber/resume f))) 0]
   ["alloc-return"
    (fn [j]
      (let [f (fiber/new (fn [] (string "v-" j)) 1)]
        (fiber/resume f))) 0]
   ["fiber-nested"
    (fn [j]
      (let [f (fiber/new (fn []
                           (let [g (fiber/new (fn [] j) 1)]
                             (fiber/resume g))) 1)]
        (fiber/resume f))) 0]
   ["multi-resume"
    (fn [j]
      (let [f (fiber/new (fn []
                           (yield 1)
                           (yield 2)
                           3) |:yield|)]
        (fiber/resume f)
        (fiber/resume f)
        (fiber/resume f))) 0]
   ["protect-call"
    (fn [j]
      (let [[ok v] (protect (+ 1 2))]
        v)) 0]
   ["yield-discard"
    (fn [j]
      (let [f (fiber/new (fn []
                           (yield {:x j})
                           99) |:yield|)]
        (fiber/resume f))) 0]
   ["never-resumed"
    (fn [j]
      (let [f (fiber/new (fn [] {:x j}) |:yield|)]
        f)) 0]
   ["denied-discard"
    (fn [j]
      (let [f (fiber/new (fn [] (println "blocked")) |:error :io| :deny |:io|)]
        (fiber/resume f)
        (get (fiber/value f) :error))) 3]  # A parked fiber hard-killed by `fiber/cancel` reclaims fully: the kill
   # frees everything the fiber owns (owner nodes, the parked signal's park
   # escape retain), and no carrier retain pins the fiber region
   # (docs/impl/region/owner.md § "Park/unpark symmetry").
   ["cancel-discard"
    (fn [j]
      (let [f (fiber/new (fn []
                           (yield j)
                           9) |:yield|)]
        (fiber/resume f)
        (fiber/cancel f :dead)
        (fiber/status f))) 0]  # `fiber/abort` of a PARKED fiber (F2). Abort injects an error
   # and resumes the fiber for unwinding; with no in-body handler it lands `:error`,
   # which the model keeps RESUMABLE (the restarts system), so its re-parked frame
   # strands the DEAD CONTINUATION's pending value releases — the borrowed tail
   # arg's retain from the abort call's error exit, and call-slot scratch — which
   # only a restart replay could consume (docs/impl/region/owner.md § "The bounded
   # residual"). The park-symmetry mechanisms (carrier, owner nodes, parked-signal
   # retain, the fiber region itself) are closed; `denied-discard` (3) is the same
   # residual class for a capability denial. Shrink-only.
   ["abort-discard"
    (fn [j]
      (let [f (fiber/new (fn []
                           (yield j)
                           9) |:yield|)]
        (fiber/resume f)
        (protect (fiber/abort f "boom")))) 4]])

# A pinned rate is an exact number (matched within ±0.5 — integer resolution on
# the real-valued estimate) or a [lo hi] inclusive range (for the rare shape
# whose true rate genuinely spans across tiers).
(defn match-rate? [got want]
  (if (array? want)
    (and (not (< got (get want 0))) (not (< (get want 1) got)))
    (and (< (- got want) 0.5) (< (- want got) 0.5))))

# pin is the single assertion shape for every class — table-driven or
# bespoke. Shrink-only: a fix LOWERS the pin.
(defn pin [r want]
  (show r)
  (check (assert (match-rate? (get r :rate) want)
                 (string (get r :label) ": pinned " want ", measured "
                         (get r :rate) " (" (get r :verdict) ") — shrink-only"))))

(println "── folded suite: direct-loop class ──")
(each entry suite-classes
  (pin (measure (get entry 0) (get entry 1) 100 6 60 0.4 0.5) (get entry 2)))

# ── Tail-call rotation ────────────────────────────────────────────────
# The loop IS the recursion, so the run-block is the recursive call itself: one
# call with arg b performs b allocations via tail recursion. Tail-call rotation
# (not while-scope) is the mechanism that must reclaim them — so it gets its own
# driver. n varies the input so a body cannot constant-fold.
# All four recur fns are passed as fn-values into measure-core, so no visible
# call site can prove `n` and call-site param joins do not fire; a local
# diverging guard proves each %sub operand instead (docs/intrinsics.md § The
# contract). Contrast lcl-self below, which is called directly and needs no
# guard. The guard never fires on the driver's int inputs and holds no heap arg,
# so the measured tails are undisturbed at 0/op.
(defn struct-recur [n]
  (when (%not (%int? n)) (error :struct-recur-nan))
  (if (= n 0)
    nil
    (begin
      {:x n}
      (struct-recur (%sub n 1)))))
(defn string-recur [n]
  (when (%not (%int? n)) (error :string-recur-nan))
  (if (= n 0)
    nil
    (begin
      (string "iter-" n)
      (string-recur (%sub n 1)))))
(defn odd-recur [n]
  (when (%not (%int? n)) (error :odd-recur-nan))
  (if (= n 0)
    nil
    (begin
      {:parity :odd}
      (even-recur (%sub n 1)))))
(defn even-recur [n]
  (when (%not (%int? n)) (error :even-recur-nan))
  (if (= n 0)
    nil
    (begin
      {:parity :even}
      (odd-recur (%sub n 1)))))
(println "── folded suite: tail-call rotation ──")
(pin (measure-core "recur-struct" struct-recur count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "recur-string" string-recur count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "recur-mutual" even-recur count-gauge 100 6 60 0.4 0.5) 0)

# ── Letrec-local recursive closures — self (cell-free) vs mutual (cycle) ──
# A `letrec`-bound recursive closure NESTED in a function body — the UNIVERSAL shape:
# every recursive local helper, every variadic operator (`+`/`<` build a
# `(letrec [go …] …)` over their varargs). The `recur-*` probes above do NOT cover it
# (a top-level `defn` calling itself takes a different path).
#
# SELF-recursion (`recur-local-self`) is reclaimed (rate 0): a self-recursive `loop` is
# CELL-FREE — its self-edge does not mark it captured, so there is no forward cell and no
# cell↔closure cycle; its self-reference resolves to the executing closure (`LoadSelf` /
# a self-call), RC-identical to a top-level recursive `defn` (docs/impl/selfrec.md). The
# per-call closure region is stranded past the recursive `TailCall` and reclaimed by the
# tail-call deferred release (lir/lower/control/call.rs `tail_callee_defers_release`). The HOF pins above
# (map/reduce/zip/…) ride this same cell-free mechanism — their `go` helpers.
#
# MUTUAL recursion (`recur-local-mutual`) is reclaimed (rate 0): `ev`/`od` each capture
# the OTHER, a genuine closure↔closure cell cycle — but an immutable lambda-initialized
# letrec binding's forward cell is a compiled static-slot cell in every position, so the
# closure-cycle merge collapses the SCC + cells onto one arena in-lambda exactly as at
# top level. The tail-call letrec body `(ev n)` strands the binding-scope drop; the
# tail-call deferred release releases the merged arena once at the recursion's normal completion
# (docs/impl/region/letrec.md § The letrec closure-cycle merge).
(defn lcl-self [n]
  (letrec [go (fn [m] (if (%lt m 1) :done (go (%sub m 1))))]
    (go n)))
(defn lcl-mutual [n]
  (letrec [ev (fn [m] (if (%lt m 1) :even (od (%sub m 1))))
           od (fn [m] (if (%lt m 1) :odd (ev (%sub m 1))))]
    (ev n)))
(println "── folded suite: letrec-local recursive closures ──")
(pin (measure "recur-local-self" (fn [j] (lcl-self 3)) 100 6 60 0.4 0.5) 0)
(pin (measure "recur-local-mutual" (fn [j] (lcl-mutual 3)) 100 6 60 0.4 0.5) 0)

# NON-member body tail — the same ev/od cycle, but the letrec BODY ends in a tail call
# to a NON-member. `(ev n)` above is a tail call to a MEMBER (its stranded binding-scope
# drop rides `stranded_cycle_bindings`); here `(%add (ev n) 0)` (an inline opcode
# whose operand is the call) and `(+ (ev n) 0)` (the stdlib redefines `+`
# to a bytecode CLOSURE) end in a frame-replacing tail call to a non-member.
# That strands the merged arena's binding-scope drop as dead code, so the
# release rides the explicit arena adopt (`TailCall::deferred_release_slot`,
# `RegionInfo::cycle_tail_release`): a closure callee (`+`) adopts the arena at
# the recursion's completion, a native callee (`%add`) never replaces
# the frame and falls through to the live scope-exit drop — mutually exclusive per call,
# so exactly one release fires however the callee resolves. Both reclaim (rate 0); the
# closure-cycle merge previously REFUSED a non-member-tail clique, leaving it Shared and
# leaking its whole arena ~4/op (docs/impl/region/letrec.md § The letrec closure-cycle
# merge). The base cases return 0/1 so `(%add (ev n) 0)` is well-typed.
(defn lcl-mutual-native [n]
  (letrec [ev (fn [m] (if (%lt m 1) 0 (od (%sub m 1))))
           od (fn [m] (if (%lt m 1) 1 (ev (%sub m 1))))]
    (%add (ev n) 0)))
(defn lcl-mutual-op [n]
  (letrec [ev (fn [m] (if (%lt m 1) 0 (od (%sub m 1))))
           od (fn [m] (if (%lt m 1) 1 (ev (%sub m 1))))]
    (+ (ev n) 0)))
(pin (measure "recur-local-mutual-native" (fn [j] (lcl-mutual-native 3)) 100 6
              60 0.4 0.5) 0)
(pin (measure "recur-local-mutual-op" (fn [j] (lcl-mutual-op 3)) 100 6 60 0.4
              0.5) 0)

# ── Retained-closure reclamation (a self-reference cycle, F4) ──
# `recur-local-self` above pins the LEAK rate of a self-recursive closure used as a
# LOOP (0 — cell-free, reclaimed per call). These two RETAIN each returned closure in
# a block-local @keep, so the question becomes whether the closure's own region
# reclaims when @keep is freed at the block's return.
#
# The foreign-capture CONTROL (`lcl-foreign-ret` captures the immediate n, not itself)
# is acyclic and reclaims to 0: its closure+env free with @keep. The self-returning
# closure (`lcl-self-ret` returns `go`, which references itself) is a genuine reference
# cycle — `go`'s reachability includes `go` — so RC cannot collect it and it stays
# Shared even after @keep frees (F4 cyclic refusal-to-Shared), pinning its closure+env
# at 2/op. The gap (self 2, foreign 0) isolates the self-reference cycle; it was masked
# while the F1b `push` over-keep held @keep itself alive across the block, reading both
# at 2. Object growth, not region growth, is the gauge (closure + env share one region).
# The self-recursive LOOP being cell-free is a distinct property, pinned deterministically
# by runtime::tests::ownership::self_recursive_loop_is_cell_free.
(defn lcl-self-ret [n]
  "Self-recursive local closure that RETURNS itself (so a retain pins its region)."
  # go is returned (value position), which disables call-site param joins, so a
  # local diverging guard proves the %lt/%sub operands (as in lcl-foreign-ret).
  (letrec [go (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) go (go (%sub m 1))))]
    (go n)))
(defn lcl-foreign-ret [n]
  "Equal-arity cell-free control: captures the immediate n, not itself."
  # h is only returned (no in-file call sites), so m is untyped without the
  # (numeric!) declaration.
  (let [h (fn [m]
            (when (%not (%int? m)) (error :m))
            (if (%lt m 1) n n))]
    h))
(defn retain-block [mk]
  "Run-block: build (mk) b times into a block-local @keep so each pinned closure's
   region — and the cell it holds — stays live, exposing the per-call mint."
  (fn [b]
    (when (%not (%int? b)) (error :block-not-int))
    (def @keep @[])
    (def @j 0)
    (while (%lt j b)
      (push keep (mk))
      (assign j (%add j 1)))))
(pin (measure-core "recur-local-self-mint"
                   (retain-block (fn [] (lcl-self-ret 3))) count-gauge 100 6 60
                   0.4 0.5) 2)
(pin (measure-core "recur-local-foreign-mint"
                   (retain-block (fn [] (lcl-foreign-ret 3))) count-gauge 100 6
                   60 0.4 0.5) 0)

# ── Stdlib / native-tail / discarded-tail leak classes ────────────────
# Three more leak classes pinned in the one dashboard (leak state
# read in one place). Each pin is the TRUE CURRENT rate, shrink-only: a fix LOWERS
# it.
#
# `region-gauge` (arena/region-count) is the second heap dimension — a class can
# leak whole REGIONS without growing the object count (a native fresh-result
# region whose contents are few). `stmt-run` drives a thunk b times as a discarded
# STATEMENT (non-tail), the while-loop shape a per-call leak needs to surface.
(defn region-gauge []
  (arena/region-count))
(defn stmt-run [thunk]
  (fn [b]
    (when (%not (%int? b)) (error :block-not-int))
    (def @i 0)
    (while (%lt i b)
      (thunk)
      (assign i (%add i 1)))))
# Native pass-through / closure tail-returns, for the discarded-tail-return class.
(defn ora-ret-first [xs]
  (first xs))
(defn ora-mk [x]
  {:v x})
(defn ora-ret-closure [x]
  (ora-mk x))

(println "── folded suite: stdlib / native-tail / discarded-tail canaries ──")

# Stdlib per-call leak (F1a — the transform-scratch retain). The leaked
# objects are INTERMEDIATE scratch, NOT the recursive helper (which reclaims — the
# `recur-local-*` probes read 0) and NOT, mostly, cons cells. Stage 1 dissolved the
# first/rest copy-scratch AND the per-call `go` closure: `fold`/`reduce` now
# `(->array coll)` once and INDEX-walk through the shared self-recursive
# `core-fold-step` driver (core.lisp) — `fold`/`reduce` read 0 with a fresh-lambda
# combiner. `stdlib-fold`'s residual 1 is the heap accumulator element the reducer
# threads forward. This rate is the SOUND one: the driver releases the
# tail-transferred accumulator exactly once. A transiently-lower reading came from
# a latent double-free of that accumulator (the same over-free that SIGSEGVs
# stdlib `compose`/`comp` — pinned by
# tests/integration/fixtures/region-compose-closure-acc-uaf.lisp); it decremented
# one extra region per fold, an unsound reclamation, not a real one. `concat` builds
# a fresh accumulator + per-arg combiner closures. All non-escaping, acyclic
# call-result regions no static slot can name. Pinned at the exact `(concat "a" "b")`
# / 2-element `fold` shapes.
(pin (measure-core "stdlib-concat" (stmt-run (fn [] (concat "a" "b")))
                   count-gauge 100 6 60 0.4 0.5) 3)
(pin (measure-core "stdlib-fold"
                   (stmt-run (fn [] (fold (fn [_ b] b) nil (list "x" "y"))))
                   count-gauge 100 6 60 0.4 0.5) 1)

# ── HOF-composition dissolution debt — the zip-tower witness ───────────
# `zip-tower` is a zip built as a TOWER of higher-order calls: it converts every
# input to a list (`map to-list`), then recurses building the result with `(map
# first lists)` AND `(map rest lists)` at every step, then rebuilds an array. It is
# `map`/`pair`/`reverse`/`push` stacked several deep — and it leaks ~25 objects per
# `(zip-tower [1 2] [3 4])` where a direct index walk over the same inputs leaks ~10
# (the `zip` probe above, the production form). That ~15-object gap is not zip-specific:
# it is the general fact that COMPOSING higher-order calls COMPOUNDS the collection-builder
# over-keep — each layer's fresh accumulator/closures leak, so the total scales with
# composition DEPTH, not per-call constant. This is the shape the north star says the
# compiler should DISSOLVE: fuse the map-of-map into one loop, leaving no intermediate
# collections to leak. Until dissolution loop-ifies HOF composition, this probe pins the
# debt; it closes when a towered composition reclaims to the same floor as the hand-fused
# form — NOT by hand-rewriting each composition, which is the programmer bridging a gap the
# compiler should close. (The production `zip` WAS so rewritten, for the RSS win; this probe
# is the standing record of what that rewrite worked around.) Shrink-only, pinned as a
# CROSS-TIER RANGE [25 32]: the layers' arg-position closure-call results ride the
# now-unconsumed ReturnValue retain (the `arg-result` class, § F5) at composition depth on
# the VM (32), but the JIT does not hold that retain (25) — a genuine VM/JIT span, so the
# pin is the [lo hi] range that greens both tiers. Both bounds are shrink-only: a fix lowers
# them (and collapses the range once the arg-retain gap closes and the tiers reconverge).
(defn zip-tower [& colls]
  (letrec [to-list (fn (c)
                     (cond
                       (or (pair? c) (empty? c)) c
                       (array? c)
                         (letrec [loop (fn (i acc)
                                         (if (>= i (length c))
                                           (reverse acc)
                                           (loop (+ i 1) (pair (get c i) acc))))]
                           (loop 0 ()))
                       true (error {:error :type-error
                                    :reason :not-a-sequence
                                    :message "not a sequence"})))
           from-list (fn (lst orig)
                       (if (array? orig)
                         (let [arr @[]]
                           (each x in lst
                             (push arr x))
                           arr)
                         lst))
           zip-lists (fn (lists)
                       (if (any? empty? lists)
                         ()
                         (pair (map first lists) (zip-lists (map rest lists)))))]
    (if (empty? colls)
      ()
      (let* [lists (map to-list colls)
             result (zip-lists lists)]
        (from-list result (first colls))))))
(pin (measure-core "zip-tower" (stmt-run (fn [] (zip-tower [1 2] [3 4])))
                   count-gauge 100 6 60 0.4 0.5) [24 32])

# Dispatch-wrapper IMMUTABLE-input residual — CLOSED by cross-unit monomorphization
# (F1b; `hir/typeinfer/monomorphize.rs`). `put`/`del` on an immutable
# aggregate used to route through the whole wrapper — a `(match (type-of coll) …)` that
# used `coll` in EVERY arm with a single `decref_point` in one, stranding the owned-param
# container reference on the other paths PLUS a redundant fresh-result retain. The
# wrapper's definition lives in the stdlib unit, so the intra-unit monomorphize pass
# never reached a user call and only the container half was recoverable (by compensation).
# The cross-unit dispatch-wrapper registry now collapses `(put {…} …)` to the direct
# `%put-struct` at the proven immutable type — the wrapper, and every strand it carried,
# cease to exist, with no compensation gate. These are now CLOSED controls pinning that
# collapse (the one arm the registry leaves alone is a MUTABLE in-place `del`, which stays
# on its container compensation — `monomorphize.rs`, `is_mutable_container`).
(pin (measure-core "native-tail-put-struct" (stmt-run (fn [] (put {:a 1} :b 2)))
                   region-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "native-tail-put-array" (stmt-run (fn [] (put [10 20] 0 99)))
                   region-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "native-tail-del-ctl" (stmt-run (fn [] (del {:a 1 :b 2} :a)))
                   region-gauge 100 6 60 0.4 0.5) 0)
# The store family beyond `put`/`del`: `push`/`add` on an immutable container had the
# SAME cross-unit wrapper strand, and leaked identically (measured 1/op for array/set,
# 2/op for the byte-copy string push, with the cross-unit path disabled) — but the F1b
# probe set never covered them, so the leak sat in the oracle's blind spot until the
# same registry collapse closed it. These are CLOSED controls pinning that the store
# family is handled generically, not just `put`.
(pin (measure-core "native-tail-push-array" (stmt-run (fn [] (push [1 2] 3)))
                   region-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "native-tail-add-set" (stmt-run (fn [] (add (set 1 2) 3)))
                   region-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "native-tail-push-string" (stmt-run (fn [] (push "ab" "c")))
                   region-gauge 100 6 60 0.4 0.5) 0)
# The MUTABLE fresh-result funnels — `push` on a mutable @bytes (`%bytes-push`, no
# in-place variant, returns fresh) and `pop` on a mutable @string (`%pop-string`,
# returns a fresh grapheme). Their raw ops reclaim (0/op direct), but the polymorphic
# wrapper stranded the mutable container: NOT a pass-through funnel, so the container
# compensation (which closes `%push-array-mut`/`%put-struct-mut`) never covered them.
# A matrix-coverage gap the whole-family sweep surfaced (push/pop on every type×
# mutability). Closed by extending cross-unit monomorphization to every self-reclaiming
# op on any mutability — only the mutable in-place `%del-*-mut` (open F5) is held back.
(pin (measure-core "native-tail-push-mut-bytes"
                   (stmt-run (fn [] (push (@bytes 1 2) 3))) region-gauge 100 6
                   60 0.4 0.5) 0)
(pin (measure-core "native-tail-pop-mut-string"
                   (stmt-run (fn [] (pop (@string "abc")))) region-gauge 100 6
                   60 0.4 0.5) 0)

# ── The read/copy class ───────────────────────────────────────────────
# The container READ and single-value COPY primitives — `first`/`rest`/`get`/`has?`/
# `length`/`last`/`->array`/`->list`/`keys`/`values`/`slice`. Every one RECLAIMS
# (0/op): the F1a copy-scratch leak is COMPOSITIONAL — it lives in the HOF/transform
# BODIES (`take`/`drop`/`reverse`/`concat`/`merge`/`distinct`/…, pinned in the F1a
# suite above), never in a standalone read of a discarded result, even the tail-COPY
# `(rest arr)`. These are CLOSED controls: the family was previously unpinned, an
# oracle blind spot the whole-matrix sweep (the same audit that found the push/add and
# push-mut-bytes/pop-mut-string gaps) closed. A regression that makes any read
# primitive strand its result fails here loud.
(pin (measure-core "read-first" (stmt-run (fn [] (first [1 2 3]))) region-gauge
                   100 6 60 0.4 0.5) 0)
(pin (measure-core "read-rest" (stmt-run (fn [] (rest [1 2 3]))) region-gauge
                   100 6 60 0.4 0.5) 0)
(pin (measure-core "read-last" (stmt-run (fn [] (last [1 2 3]))) region-gauge
                   100 6 60 0.4 0.5) 0)
(pin (measure-core "read-get-array" (stmt-run (fn [] (get [1 2 3] 0)))
                   region-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "read-get-struct" (stmt-run (fn [] (get {:a 1} :a)))
                   region-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "read-has-struct" (stmt-run (fn [] (has? {:a 1} :a)))
                   region-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "read-length" (stmt-run (fn [] (length [1 2 3])))
                   region-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "read-toarray" (stmt-run (fn [] (->array (set 1 2))))
                   region-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "read-tolist" (stmt-run (fn [] (->list [1 2 3])))
                   region-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "read-keys" (stmt-run (fn [] (keys {:a 1 :b 2})))
                   region-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "read-values" (stmt-run (fn [] (values {:a 1 :b 2})))
                   region-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "read-slice" (stmt-run (fn [] (slice [1 2 3 4] 1 3)))
                   region-gauge 100 6 60 0.4 0.5) 0)

# Discarded tail-return: a function whose tail is a call (native pass-through
# `first`, or a closure), invoked for effect with the result DISCARDED. The
# fresh-value pass-through RECLAIMS (the move convention balances) — pinned closed
# here; the residual is a jit-only reclamation gap for a STDLIB-allocated (`concat`)
# result, the `stdlib-concat` pin above.
(pin (measure-core "discard-passthrough"
                   (stmt-run (fn [] (ora-ret-first (list {:k 1})))) count-gauge
                   100 6 60 0.4 0.5) 0)
(pin (measure-core "discard-closure" (stmt-run (fn [] (ora-ret-closure 1)))
                   count-gauge 100 6 60 0.4 0.5) 0)

# ── The mutable-store funnel — remove/rebind half ─────────────────────
# The store half (push/put/add) is pinned above (push-churn/struct-put/set-array/…);
# these pin the REMOVE and REBIND half of the same seam (docs/impl/region/ownership.md
# § "The outgoing edge table"; src/value/arena/mutate.rs). The funnel SEAM is
# complete-by-construction — every remove co-locates its RC decref with the outgoing
# un-record, the raw accessors are private (an uncounted store is a compile error), and
# a debug equivalence oracle asserts the recorded table matches a content scan at every
# free. These pins read the seam THROUGH the surface that reaches it, and split cleanly:
#
#   `%pop` — the remove funnel balances (rate 0): a box store+rebind and
#   `%pop`'s `moves_out` native each reclaim their cross-region member, so `raw-pop` is
#   the reclaiming CONTROL (the peer of push-slot-source/put-slot-source) proving the
#   remove funnel sound, and a wrapper that leaks over it is the wrapper's leak. It is a
#   DIRECT while-statement, not a thunk: the popped value is discarded as a statement, so
#   it isolates the remove funnel's own reclamation from the return convention and from
#   the ownership forest's handling of a value pushed into a LOCAL then popped OUT and
#   RETURNED. `%pop` is a native call whose result is a distinct `call_result` region
#   with its own `DecrefValueRegion`, which balances the `moves_out` retain
#   (`pop_with_decref`) that hands the element back.
#
#   F1b remove-wrapper — the stdlib `pop`/`del` `(match (type-of coll)
#   …)` dispatch wrapper strands the container arg + fresh result on the arms the
#   textually-last arm does not reach, exactly as the STORE wrappers (put/push/set) do.
#   `pop` leaks (3): the leak is the multi-arm wrapper. Closes by the SAME mechanism as
#   the store half — per-arm compensation of the container+result, or dispatch prune on a
#   statically-typed scrutinee.
#
#   The RAW remove funnel reclaims too (`raw-del`/`raw-del-immediate` = 0): `%del`'s
#   in-place @struct/@set remove decrefs the removed member and its `-mut` pass-through
#   result carries exactly one return mint. These two are the CLOSED raw-funnel controls
#   for the remove half, the peers of `raw-pop`/`put-slot-source`. Their probe shape is
#   deliberately a two-statement body whose tail is the funnel call — the ANF-named tail
#   call whose result a `Return` mint covers (docs/impl/region/mechanism.md § "The return
#   mint is emitted exactly once") — so a second, unbalanced retain there reads here as a
#   whole stranded container plus the member it holds.
(println "── folded suite: mutable-store funnel (remove/rebind half) ──")
(pin (measure-core "box-rebind"
                   (stmt-run (fn []
                               (let [b (box (list 1 2))]
                                 (rebox b (list 3 4))))) count-gauge 100 6 60
                   0.4 0.5) 0)
# F1b — the stdlib `add` `(match (type-of coll) …)` dispatch
# wrapper reclaims its owned @set container AND its stored heap member (rate 0): the
# `:@set` arm's `%add-set-mut` returns the container pass-through, and the wrapper's
# per-arm container release (`regions::compensate`, `funnel_container_sites`) frees
# the stranded owned-param reference, cascading the stored list through the outgoing
# edge table. A CLOSED control beside the reclaiming raw funnel `set-add-slot-source`
# — RED if the container compensation regresses.
(pin (measure-core "set-add"
                   (stmt-run (fn []
                               (let [s @||]
                                 (add s (list 1 2))))) count-gauge 100 6 60 0.4
                   0.5) 0)
(pin (measure-core "raw-pop"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (let [a @[]]
                         (%array-push a (%pair 1 2))
                         (%pop a))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
# The stdlib `pop` REMOVE-of-ELEMENT wrapper reclaims (rate 0). Its `:@array`/
# `:@string`/`:@bytes` arms route to the monomorphic moves-out funnels
# `%pop`/`%pop-string`/`%pop-bytes`; the container compensation frees the wrapper's
# stranded owned-param container per-arm (recorded for a moves-out funnel even though
# it returns the ELEMENT, not the container), and the moved-out @array element's
# redundant tail ReturnValue retain is suppressed (`moves_out_release_sites`) — so
# both halves of the earlier leak close.
(pin (measure-core "pop-wrapper"
                   (stmt-run (fn []
                               (let [a @[]]
                                 (push a (list 1 2))
                                 (pop a)))) count-gauge 100 6 60 0.4 0.5) 0)
# The stdlib `del` REMOVE wrapper reclaims (rate 0), the remove-half peer of the
# store wrappers: its `:@struct`/`:@set` arms route to the `-mut` remove funnels
# (`%del-struct-mut`/`%del-set-mut`) that return the container pass-through, and the
# wrapper's container compensation frees the stranded owned-param reference — a
# CLOSED control beside the reclaiming raw funnel `put-slot-source`.
(pin (measure-core "del-wrapper"
                   (stmt-run (fn []
                               (let [m @{}]
                                 (put m :k (list 1 2))
                                 (del m :k)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "set-del-wrapper"
                   (stmt-run (fn []
                               (let [s @||]
                                 (add s 7)
                                 (del s 7)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "raw-del"
                   (stmt-run (fn []
                               (let [m @{}]
                                 (%put m :k (%pair 1 2))
                                 (%del m :k)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "raw-del-immediate"
                   (stmt-run (fn []
                               (let [m @{}]
                                 (%put m :k 7)
                                 (%del m :k)))) count-gauge 100 6 60 0.4 0.5) 0)

# ── Fiber-internal yielding loops ─────────────────────────────────────
# The loop and the yield live inside the fiber. The run-block creates a fiber
# that runs b internal iterations then COMPLETES, and drains it — so loop-scope
# reclamation fires exactly as in the originals (a forever-generator never exits
# its loop, so per-iteration values that reclaim at scope-exit would falsely read
# as leaks). Flip rotation at the yield back-edge is the mechanism under test.
(defn drain-block [make b]
  (let [f (make b)]
    (while (not= (fiber/status f) :dead) (fiber/resume f))))
(defn yielding-fiber [body]
  "(fn [n]) → a fiber that runs (body i), yields, n times, then completes."
  (fn [n]
    (when (%not (%int? n)) (error :n-not-int))
    (fiber/new (fn []
                 (def @i 0)
                 (while (%lt i n)
                   (body i)
                   (yield i)
                   (assign i (%add i 1)))) |:yield|)))
(defn pin-yield [label body rate]
  (pin (measure-core label (fn [b] (drain-block (yielding-fiber body) b))
                     count-gauge 100 6 60 0.4 0.5) rate))
(println "── folded suite: fiber-internal yield ──")
(pin-yield "yield-struct" (fn [i] {:x i}) 0)
(pin-yield "yield-string" (fn [i] (string "iter-" i)) 0)
(pin-yield "yield-closure"
           (fn [i]
             (let [f (fn [] i)]
               (f))) 0)
(pin-yield "yield-concat" (fn [i] (concat "x" (number->string i))) 3)
(pin (measure-core "yield-put"
                   (fn [b]
                     (drain-block (fn [n]
                                    (when (%not (%int? n)) (error :n-not-int))
                                    (fiber/new (fn []
                                      (def @st @{:data nil})
                                      (def @i 0)
                                      (while (%lt i n)
                                        (put st :data {:iter i})
                                        (yield i)
                                        (assign i (%add i 1)))) |:yield|)) b))
                   count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "yield-reassign"
                   (fn [b]
                     (drain-block (fn [n]
                                    (when (%not (%int? n)) (error :n-not-int))
                                    (fiber/new (fn []
                                      (def @v (string "init"))
                                      (def @i 0)
                                      (while (%lt i n)
                                        (assign v (string "val-" i))
                                        (yield i)
                                        (assign i (%add i 1)))) |:yield|)) b))
                   count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "yield-multimut"
                   (fn [b]
                     (drain-block (fn [n]
                                    (when (%not (%int? n)) (error :n-not-int))
                                    (fiber/new (fn []
                                      (def @sess
                                        @{:count 0 :last nil :streams @{}})
                                      (def @i 0)
                                      (while (%lt i n)
                                        (let [frame {:type :data
                                          :stream-id i
                                          :payload (string "p-" i)}]
                                          # field reads are untyped; the
                                          # allocation-free guard proves the
                                          # %add operand
                                          (let [c sess:count]
                                            (when (%not (%int? c))
                                              (error :count-not-int))
                                            (put sess :count (%add c 1)))
                                          (put sess :last frame)
                                          (put sess:streams i frame))
                                        (yield i)
                                        (assign i (%add i 1)))) |:yield|)) b))
                   count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "yield-spawn"
                   (fn [b]
                     (drain-block (fn [n]
                                    (when (%not (%int? n)) (error :n-not-int))
                                    (fiber/new (fn []
                                      (def @i 0)
                                      (while (%lt i n)
                                        (let [label (string "task-" i)
                                          f (fiber/new (fn []
                                            (string label "-done")) |:yield|)]
                                          (fiber/resume f))
                                        (yield i)
                                        (assign i (%add i 1)))) |:yield|)) b))
                   count-gauge 100 6 60 0.4 0.5) 0)

# ── Channel send/recv — the genuinely-Shared (class 7) incoming-count ──
# `chan/send` is the sole `RegionEffect::Sends` declarant: its message crosses the
# fiber frontier (it rides the channel buffer, by pointer, to the receiving fiber),
# so it can never be Owned by a bounded activation and stays on the incoming-count
# (per-region RC) path — the always-Shared class. The `Sends` edge increfs the
# message region at the send site to hold it in the buffer until received ("a store
# into a Shared region bumps its count"); the receive removes it from the buffer, so
# its region's incoming count is lowered there ("an overwrite/drop lowers it" —
# region/ownership.md § class 7, the Shared incoming-count). Reclaimed: rate 0. The
# fresh channel each block is created and freed within the run-block, so only the
# per-op message reclamation shows; RED (2/op) without the receive-side release.
(pin (measure-core "chan-send-recv"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (let [[s r] (chan)]
                       (def @i 0)
                       (while (%lt i b)
                         (chan/send s {:k i :v (string "v" i)})
                         (chan/recv r)
                         (assign i (%add i 1))))) count-gauge 100 6 60 0.4 0.5)
     0)

# ── Persistent fn-local containers ────────────────────────────────────
# The container is `def`'d fn-local INSIDE the run-block (the faithful shape — a
# captured let-local or module binding hits a different region path) and reused
# across the block's ops. `push-outer` reclaims (rate 0): a block-local accumulator
# is freed at the block's return once the `push` wrapper stops stranding its
# owned-param reference (F1b container compensation) — its earlier per-op growth was
# that over-keep, not genuine retention (the gauge-live discriminator uses a
# MODULE-level sink, `probe-disc`, and is unaffected). `push-accum` still leaks: its
# residual is the per-op `map` scratch (§ F1a), which the accumulator's release does
# not reach. Its kernel CAPTURES `k` deliberately — a capture declines loop fusion,
# so the real stdlib `map` runs and there is a per-op scratch to retain; a fusable
# kernel has none, which is what the dissolution controls (`map-while`) measure.
# `struct-outer` is the fn-local reassign-1-slot control: a loop-carried cell whose
# content is re-minted every iteration, bounded by the overwrite + demise pair (F5).
# `string-outer`/`append-outer` are the `concat`/`append` per-call scratch leak
# (§ F1a), NOT accumulator growth — flat per-iter (minus the 1 the self-reassign
# reclaims), so they shrink when F1a closes, not when the loop ends.
(println "── folded suite: persistent containers ──")
(pin (measure-core "put-overwrite"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @s @{:key 0})
                     (def @j 0)
                     (while (%lt j b)
                       (put s :key (string "v" j))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "set-array"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @a @[(string "i")])
                     (def @j 0)
                     (while (%lt j b)
                       (put a 0 (string "v" j))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "put-struct"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @s @{:data nil})
                     (def @j 0)
                     (while (%lt j b)
                       (put s :data {:iter j})
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "roster"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @tr @{:pnl 0 :trades 0 :label ""})
                     (def @j 0)
                     (while (%lt j b)
                       (put tr :pnl (%add j 100))
                       (put tr :trades (%add j 1))
                       (put tr :label (string "t-" j))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "put-outer"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @s @{:x 0})
                     (def @j 0)
                     (while (%lt j b)
                       (put s :x (string "v" j))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "push-outer"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @acc @[])
                     (def @j 0)
                     (while (%lt j b)
                       (push acc {:x j})
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "push-accum"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @acc @[])
                     (def @j 0)
                     (def k 1)
                     (while (%lt j b)
                       (push acc
                             (map (fn [x]
                                    (numeric!)
                                    (%add x k)) [1 2 3]))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 3)
(pin (measure-core "struct-outer"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @last nil)
                     (def @j 0)
                     (while (%lt j b)
                       (assign last {:x j})
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "string-outer"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @s "")
                     (def @j 0)
                     (while (%lt j b)
                       (assign s (concat s "x"))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 3)
(pin (measure-core "append-outer"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @acc [])
                     (def @j 0)
                     (while (%lt j b)
                       (assign acc (append acc [j]))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 3)

# ── Discarded call-result + break-escape ──────────────────────────────
# Direct while-statement run-blocks (no thunk wrapper): the discarded value is a
# CALL-RESULT (Rule 2's discarded-result release), and a thunk's return
# convention would reclaim the break-escape on its own, hiding what the break
# probes below are here to measure — the block, not the enclosing call, is what
# must anchor a release the break's jump passes over.
(println "── folded suite: call-result + break ──")
(pin (measure-core "branch-call"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       # %rem (not the mod wrapper): j is a proven local int
                       # and the wrapper's result would be untyped
                       (if (%lt (%rem j 2) 1) (t17-h) (t17-h2))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
# The raw `%array-push`/`%put` into a fresh container, discarded: the CONTROL for
# F1b — the dispatch-wrapper passthrough leak. The raw intrinsic
# reclaims the container in BOTH intrinsics modes (rate 0), so the over-keep
# `put-churn` shows below (2/op) rides the stdlib `put`/`push` type-dispatch WRAPPER,
# not the store funnel. Direct while-statements (a thunk wrapper's return convention
# would inflate the rate by 1).
(pin (measure-core "push-slot-source"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (let [items @[]]
                         (%array-push items (%pair 1 2)))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "put-slot-source"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (let [s @{}]
                         (%put s :k (%pair 1 2)))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
# The raw `%add-set-mut` into a fresh @set, discarded — the set-family CONTROL
# for F1b, the peer of push-slot-source/put-slot-source. The raw silent intrinsic
# reclaims the container (rate 0), so the `set-add` over-keep (3/op) rides the
# stdlib `add` type-dispatch WRAPPER, not the set-add funnel (`set_add_with_incref`).
(pin (measure-core "set-add-slot-source"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (let [s @||]
                         (%add-set-mut s (%pair 1 2)))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
# put-churn mints a FRESH @struct container per op and hands it through the stdlib
# `put`; its `:@struct` arm's `%put-struct-mut` returns the container pass-through,
# and the wrapper's per-arm container release (`regions::compensate`,
# `funnel_container_sites`) frees the stranded owned-param reference, cascading the
# stored struct — rate 0 in both intrinsics modes, every tier. A CLOSED control
# beside `put-slot-source`; RED if the container compensation regresses.
(pin (measure-core "put-churn"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (let [s @{}]
                         (put s :k {:v j}))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
# Per-arm compensation over a `Match`, both faces. `match-dead-arm` is a CLOSED
# control: the taken arm has no use of the pre-allocated local, so the head release
# frees it (docs/impl/region/mechanism.md § "The return frontier is per-path" — the
# premises are stated over arms, so the branch's arity and kind are not read).
# `match-used-arm` is the USED face, also CLOSED: the taken arm uses the local but
# does not hold its `decref_point`, and no retain on its last-use node funds a
# per-arm release — so instead of adding one, the region's single release is
# anchored where every arm reaches it (§ "A release inside one arm is not a
# release on the other arms"). Widening `tail` to every arm-last-use node is a
# measured over-free and is NOT what closed this: an arm that used the region may
# hold an uncounted borrow the solver does not name, which is exactly why the
# close is a placement argument and not a count one.
(pin (measure-core "match-dead-arm"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t21-dead-arm :skip)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "match-used-arm"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t21-used-arm :a)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
# The owned-parameter face of the same branch structure, and its `If` twin. Both
# are CLOSED controls for the branch-arm release window: the argument's whole
# region (3 cons cells) strands on every arm that is not the one naming it last,
# unless the single release is anchored where every arm reaches it. Undeclared,
# like `rest-array-copy`, so a regression trips the completeness gate loudly
# rather than being absorbed as an F5 strand. Their counterfactual and the three
# window boundaries are `tests/elle/region-branch-arm-window.lisp`; the soundness
# complement is `region-branch-arm-window-uaf.lisp`.
(pin (measure-core "param-used-arm"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t22-param-arm (list 1 2 3) :a)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "param-used-arm-if"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t22-param-if (list 1 2 3) true)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "struct-match"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (match {:type :a :v j}
                         {:type :a :v v} v
                         _ 0)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 1)
# The frame-exit release, two CLOSED controls. A frame-replacing tail call means
# everything the lowerer emits after it runs only on the NATIVE fall-through, so a
# release landing there is emitted where control may never arrive; the close moves
# that one release ahead of the `TailCall` — admitted where escape proves the frame
# is the region's sole holder, since on the closure path it fires where none fired
# before (docs/impl/region/mechanism.md § "A release past a frame-replacing tail
# call is not a release"). `tail-frame-exit-unused` is the unused-parameter
# fallback through that dead block; `tail-frame-exit-arms` is the same strand one
# block further out, where the tail calls sit in the arms of a branch and the
# release lands past the merge; `tail-frame-exit-moved` is the exemption face,
# already 0, which reads GROWTH if the hoist ever releases an argument the callee
# now owns. Undeclared, like `param-used-arm`, so a regression trips the
# completeness gate loudly rather than being absorbed as F1a scratch. The CAPTURED
# holder — the stdlib walker, whose tail callee reaches its parameters through its
# environment — keeps the baseline and is the residual, driven as a row in
# `tests/elle/region-tail-frame-exit.lisp` rather than pinned here. That file is
# also the counterfactual; the soundness complement is
# `region-tail-frame-exit-uaf.lisp`.
(pin (measure-core "tail-frame-exit-unused"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t23-unused (list 1 2 3))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "tail-frame-exit-arms"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t23-arms (list 1 2 3) true)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "tail-frame-exit-moved"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t23-moved (list 1 2 3))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
# The three `break-value*` probes are CLOSED controls for the break TRANSFER
# (docs/impl/region/mechanism.md § "`break` transfers its value"): the value a
# `break` carries out is the BLOCK's value, so its release is anchored where the
# block's value is consumed — for a discarded block that is the block node
# itself, emitted after the exit label and reached on both paths — instead of
# inside the body the break jumps out of. Discarded, consumed, and heap-literal
# placements all reclaim; RED if the transfer regresses.
(pin (measure-core "break-value"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (block (let [x (t17-h)]
                                (break x)))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "break-value-used"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (let [r (block (let [x (t17-h)]
                                        (break x)))]
                         (get r :a))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "break-value-lit"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (block (let [x {:a j}]
                                (break x)))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
# The OTHER face of the break window, also CLOSED: a region whose value is NOT
# the one broken out, but whose `decref_point` sits between the break site and
# the block's exit label. The transfer does not reach it — the release is simply
# jumped over — so it is re-anchored to the block by the same pin
# (docs/impl/region/mechanism.md § "A release the break jumps over is not a
# release"). Its control `break-skipped-nobreak` runs the same body with the
# break unreachable, isolating the skip from the shape; RED if the window pin
# regresses. Both boundaries the window stops at — a loop or a lambda nested
# inside it — are gauged by tests/elle/region-break-skip.lisp, not here.
(pin (measure-core "break-skipped"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (block (let [x (t17-h)]
                                (when (%lt -1 j) (break 1))
                                (%struct? x)))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "break-skipped-nobreak"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (block (let [x (t17-h)]
                                (when (%lt j -1) (break 1))
                                (%struct? x)))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)

# ── Byte-gauge ────────────────────────────────────────────────────────
# Bump-arena bytes, not object count: a scope-dropped string must return its
# BYTES. Pinned as a range, shrink-only — catches a regression back to
# page-granular leaking.
(println "── folded suite: byte-gauge ──")
(pin (measure-core "string-bytes"
                   (fn [b]
                     (run-thunk-block (fn [j]
                                        (let [x (string "iter-" j
                                          "-padding-to-make-string-longer")]
                                          x)) b)) bytes-gauge 200 6 40 200.0
                   1000.0) [0 200])

# ── Value-survival correctness ────────────────────────────────────────
# Not rates — these assert a heap value SURVIVES rotation / resume, the
# correctness half of the suite the estimator does not cover.
(defn return-recur [n]
  (if (= n 0)
    (string "result-" n)
    (begin
      {:x n}
      (return-recur (%sub n 1)))))
(defn accum-recur [n acc]
  (if (= n 0) acc (accum-recur (%sub n 1) (%add acc n))))
(println "── folded suite: correctness pins ──")
(check (assert (= (return-recur 10000) "result-0")
               (string "return survives: " (return-recur 10000))))
(check (assert (= (accum-recur 10000 0) 50005000)
               (string "accumulator: " (accum-recur 10000 0))))
(check (let [fib (fiber/new (fn []
                              (def @i 0)
                              (while (%lt i 1000)
                                (yield (string "val-" i))
                                (assign i (%add i 1)))) |:yield|)
             vals (do
                    (def @acc @[])
                    (while (not= (fiber/status fib) :dead)
                      (push acc (fiber/resume fib)))
                    acc)]
         (assert (= (get vals 0) "val-0")
                 (string "yield-at-scale first: " (get vals 0)))
         (assert (= (get vals 999) "val-999")
                 (string "yield-at-scale last: " (get vals 999)))))
(check (assert (= (concat [1 2] [3 4]) [1 2 3 4]) "array concat value"))
(check (assert (= (concat "foo" "bar") "foobar") "string concat value"))

# ── The split headline — the number §1's protocol reads, printed by the tool ──
# `open defects` is the burndown count; `by-design` is the fixed growth set; `roots` is
# how many of the six declared roots still have an open probe (it falls to 0
# when the last defect closes). UNCLASSIFIED is appended only when a probe leaked
# without a declaration — a stale ledger, gated below so it can never pass silently.
(println "── split ──")
(println "open defects: " n-defects " across " (length (keys roots-seen))
         " roots; by-design: " n-by-design
         (if (= (length unclassified) 0)
           ""
           (string "; UNCLASSIFIED: " (length unclassified) " " unclassified)))
(check (assert (= (length unclassified) 0)
               (string "unclassified open probe(s): " unclassified
                       " — every open probe must be a declared root or by-design "
                       "(the split ledger is stale)")))
(check (assert (= n-by-design 3)
               (string "by-design tally " n-by-design
                       " ≠ 3 — the growth probes (live-growth discriminator, "
                       "sub-integer estimator self-test, push-accum map-scratch "
                       "accumulator) must each read open")))

(report)
(println "oracle: ok")
