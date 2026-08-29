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
(defn ids-gauge []
  (arena/region-ids))
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
# reach, and it cannot move for any shape below. An unmovable gauge paints every
# verdict green, which is what the discriminator discipline exists to refuse.

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
# of probes read open BY DESIGN — one live-growth discriminator per gauge (object count
# and physical id), and the sub-integer estimator self-test — and are NOT counted as
# defects. The open/closed
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
# `push-accum` is NOT here either, for the reason those two are not: its rate was
# the per-op `map` scratch its accumulator retained, and the accumulator itself is
# block-local, so it frees at the block's return. A CLOSED control now that the
# scratch is reclaimed — undeclared, so a regression to open trips the completeness
# gate as an F1a defect rather than being absorbed as growth.
(def @by-design
  @{"discriminator (live-growth)" true
    "id discriminator (live-growth)" true
    "sub-integer (1-in-3 retain)" true})
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
# The `concat`/`append`/`fold` shapes — `concat`, `concat-while`, `stdlib-concat`,
# `stdlib-fold`, `yield-concat`, `string-outer`, `append-outer` — are CLOSED controls
# now, and undeclared for the same reason `rest-array-copy` is. Two readings of one
# window close them: `push-all`'s bulk arm returns the accumulator its sibling arm's
# walker captures, which the branch-arm window anchors (docs/impl/region/mechanism.md
# § "The return facet costs the merge nothing"), and the index-walk fold driver
# returns its accumulator from the base arm while the recursive arm hands the callee
# the COMBINER's result — a point that cannot reach the accumulator and so owes it no
# funding edge (§ "The callee's return mint, and why the point owes it nothing"). A
# regression to open must trip the completeness gate loudly rather than be absorbed
# back into F1a.
# `group-by`, `frequencies`, `merge` and `each-list` are CLOSED controls too, under
# a different rule of the same window: one binding owns a region's release ROUTE, so
# a cursor an arm walks the input with refuses nothing (mechanism.md § "A mutated
# holder poisons its value route, not its cell box").
# `zip-tower` is a CLOSED control too, for the same reason: the tower's `letrec`
# helpers are released at a merge every dispatch arm leaves through, and the
# frame-exit relocation replicates that release into each arm through the closure's
# value route (mechanism.md § "Self-cancelling is a property of the ROUTE, not of the
# region's class").
(declare-root :f1a ["reduce" "fold"])
(declare-root :f1b ["mut-array-push" "mut-string" "struct-put" "push-churn"
                    "put-churn" "store-wrapper" "native-tail-put-struct"
                    "native-tail-put-array" "native-tail-del-ctl" "pop-wrapper"
                    "del-wrapper" "set-del-wrapper" "set-add"])
(declare-root :f2 ["fiber-nested" "multi-resume" "yield-discard"
                   "yield-multimut" "protect-while" "denied-discard"
                   "cancel-discard"])
# `abort-tail-result` and `abort-mask-caught-literal` are CLOSED controls now
# (undeclared, like `rest-array-copy`, so a regression to open trips the
# completeness gate loudly rather than being absorbed back under F2). What they
# measured was a fiber carrier in TAIL position whose outcome this fiber's mask
# absorbs: the request is answered here, so the value is the call's result and the
# frame never left — it takes the fall-through into the post-`TailCall` block, which
# runs the compiler's owned-argument releases and the return mint, exactly as the
# Call position and the JIT tier already did (docs/impl/region/mechanism.md § "A
# carrier that comes back with a result never left the frame").
# `abort-discard` is a CLOSED control now (undeclared, like `rest-array-copy`, so a
# regression to open trips the completeness gate loudly rather than being absorbed
# back under F2): what it measured was the borrowed-argument
# retain a native tail call's SIGNAL exit strands. The post-`TailCall` block that
# consumes that retain runs on the native's normal completion alone, so the exit
# consumes it instead — stamping the stash local `nil` so a replayed continuation's
# copy of the same release no-ops (docs/impl/region/mechanism.md § "What the
# fall-through owes, a signal exit owes too"). Its first stranded reference was the
# aborted fiber's own value, which pinned the body closure and everything the parked
# frame held behind it.
# `denied-discard`'s rate is what is LEFT of the dead continuation once the frames'
# own owed releases run. A frame abandoned by an error — and a parked one the fiber
# can never re-enter — reaches none of its remaining instructions, so each release
# among them runs off the value-route slots the emitter recorded
# (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it still
# owes"), gauged directly by `tests/elle/region-error-unwind.lisp`. What that walk
# cannot NAME is a value with no binding of its own, so no route and no receipt:
# the literal the denied call materialized straight into an argument, the rest list
# the calling convention built for it, and a parameter released through an env slot,
# which carries no nil stamp — so a release that ran reads like one that did not.
# `spawn-join` is a CLOSED control now (undeclared, like `rest-array-copy`), so a
# regression to open trips the completeness gate loudly rather than being absorbed
# under F2 — which is not where it belonged: what it measured was the frame-held
# admission refusing the FIBER facet, not park residue. A crossing leaves a counted
# holder — the park's `EmitEscape` retain going out, the resume value's own mint
# coming back — so the admission rides it and only the containment facets refuse
# (docs/impl/region/mechanism.md § "A fiber crossing is a counted holder too").
# `io-yield ev/sleep` is a CLOSED control (undeclared, like `rest-array-copy`): a
# pumped io op strands nothing, so a regression to open must trip the completeness
# gate loudly rather than be absorbed under a root.
# F4 has NO declared probe: the returned closure cycle is closed for every body shape,
# including the one bound OUT of its frame's tail position, where the merge follows the
# handed-out member to the release point the last-use rule already computed for it. Its
# four body shapes are CLOSED controls at 0 — `recur-local-mutual-ret` for a member tail
# call, `recur-local-mutual-ret-foreign` for a non-member one,
# `recur-local-mutual-ret-value` for a bare member value, and
# `recur-local-mutual-ret-bound` for the letrec bound out of tail position. All are
# undeclared, like `rest-array-copy`, so a regression to open trips the completeness gate
# loudly instead of being absorbed under the root. The class's other named shape — the
# ambiguous-owner / unemittable-edge subtree, the `compute_adopt_edges` refusals — still
# has no probe.
# `recur-local-self-mint` is NOT a member of this class despite the resemblance: the
# returned self-recursive closure records no region cycle at all and is cell-free, so
# it belongs to the deferred-release mechanism instead and is a control below.
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
# `struct-match` is a CLOSED control for the scope map's completeness (undeclared,
# like `rest-array-copy`): a `match` arm's pattern binding records its scope, so a
# read of it inside a loop is no longer read as a read of a loop-external binding
# and the scrutinee's release stays in the body that allocates it.
# `recur-local-self-mint` is a CLOSED control for the return-funded deferred release
# (undeclared, like `rest-array-copy`): a cell-free self-recursive closure the
# recursion RETURNS has the tail-call deferred release as its region's only channel,
# and keeps it — the callee's `Return` mints the caller's reference before the
# trampoline runs the deferred decref, so the deferral drops only the frame's own
# (docs/impl/selfrec.md § "The deferral needs no escape gate").
# Its control `recur-local-foreign-mint` is not self-recursive, so nothing strands it
# in the first place and the gap isolates the strand rather than the retain.
# `loop-acc-return` and `recur-acc-return` are CLOSED controls (undeclared, like
# `rest-array-copy`) for the RETURNED 1-slot container: a local accumulated across
# a loop and handed back counts what it holds exactly as an unreturned cell does,
# so each displaced prior dies at the overwrite and the final content leaves with
# the caller (docs/impl/region/bindings.md § "Returned fn-local reassigned
# mutables"). They must stay a PAIR: `recur-acc-return` computes the identical
# value by threading the accumulator as a parameter to a self-recursive binding,
# so the gap between them is exactly what a programmer would have to give up to be
# leak-free, and a regression of either trips the completeness gate.
(declare-root :f5 ["raw-del" "raw-del-immediate" "fresh-env-cell"])
# F6 has NO declared probe: a cursor walk's rate was the ALIASED INIT, not the
# cursor. `list-cursor` is now a CLOSED control (undeclared, like
# `rest-array-copy`) for the counted-init half of the 1-slot container: a cell
# whose init value carries a second name counts that value instead of donating
# it, so it keeps the model — and with it the store-site pin that holds each
# step's release inside the loop (docs/impl/region/bindings.md § "What the cell
# donates it must hold alone; what it counts it need not"). `each-manual` is its
# array control: an index walk names nothing it moves off and reads 0 either way,
# so the gap between the two isolates the cell rather than the loop.
# `cell-alias-after` is its ORDERING control (undeclared, like `rest-array-copy`):
# the same walk with the alias taken after the cell, so a whole-value read of the
# container takes a counted reference of its own and the cell donates its init
# (docs/impl/region/bindings.md § "A whole-value read of a 1-slot container takes
# a counted reference"). The two must stay a PAIR — each isolates one of the two
# routes the init's producer reference can take — so a regression of either trips
# the completeness gate rather than hiding behind its sibling.

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

# The ID gauge's own live-growth discriminator. The object-count discriminator
# above proves nothing about `arena/region-ids`: the two gauges move on different
# events, and an id gauge frozen at whatever the stdlib load left behind would
# read flat for every id probe below and paint each one green. The shape that
# moves it is the same genuine retain — a module-level sink keeps every region it
# is handed, so nothing is freed, the free list drains, and every later mint has
# to take a fresh id. Its own sink, not `disc-sink`: two gauges sharing one sink
# would let either probe's ops satisfy the other's gate.
(def @id-disc-sink @[])
(defn probe-id-disc [j]
  (push id-disc-sink (pair j j)))

# Bounded shape: an immutable struct built and immediately dropped — the
# reclaimed baseline (the leak suite pins this at slope 0). Should read :closed.
(defn probe-bounded [j]
  {:x j :y 2})

# A yielding io op, the whole round trip: ev/sleep is the clean shape (portless,
# nil result), so what the gauge sees is the scheduler pump's own per-op cost. The
# op suspends with an IoRequest, the pump reads a completion out of
# `(io/wait backend …)` and resumes the fiber with it, and every region on that
# path is released — the request's park retain at the resume, the completion array
# and the structs it carries at the pump's own `DecrefValueRegion`, both being one
# region (docs/impl/region/ctx.md § "A helper reached from inside a call allocates
# through THAT call's ctx"). (The IN-LAMBDA self-recursive letrec closure `+`/`<` build over
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

# 1b. The same gate for the ID gauge, which the gate above says nothing about.
(def id-disc
  (measure-core "id discriminator (live-growth)"
                (fn [b] (run-thunk-block probe-id-disc b)) ids-gauge 200 6 60
                0.4 0.5))
(show id-disc)
(check (assert (= (get id-disc :verdict) :open)
               (string "ID GAUGE DEAD: id discriminator read "
                       (get id-disc :verdict)
                       " — every id-gauge 'closed' verdict this run is void")))

# 2. Bounded baseline — must reclaim.
(def bnd
  (measure "bounded (immutable struct, dropped)" probe-bounded 200 6 60 0.4 0.5))
(show bnd)
(check (assert (= (get bnd :verdict) :closed)
               (string "bounded shape leaked: " (get bnd :verdict) " rate="
                       (get bnd :rate))))

# 3. The io round trip reclaims. Measured with the B-invariance self-test, so a
#    reading of 0 is a per-op rate rather than a per-block artifact.
(def io (measure-stable "io-yield ev/sleep" probe-io-yield 200 8 80 0.4 0.5))
(show io)
(check (assert (not= (get io :verdict) :contaminated)
               (string "io-yield rate is block-dependent (B vs 2B): "
                       (get io :rate) " vs " (get io :alt-rate)
                       " — a per-block artifact, not a per-op rate")))
(check (assert (= (get io :verdict) :closed)
               (string "io-yield leaked: " (get io :verdict) " rate="
                       (get io :rate))))

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
# The `error-payload-helper` / `error-payload-param` raisers: each allocates or
# receives the payload in a frame the error exit WALKS (the raising body's own
# frame is parked instead — that face is `error-payload`).
(defn ep-raiser [j]
  (error (string "x" j)))
(defn ep-raise-param [v]
  (error v))
# The `primitive-resume-*` bodies park at a suspending PRIMITIVE call, whose
# resume value stands in for a result no `Return` mint ever funded, so the
# delivery mints it instead (docs/impl/region/owner.md § "A delivery into a
# replayed frame carries one owning reference"). What the rate gauges is the
# mint's ARITY: one reference per delivery, consumed by the continuation's own
# result release, so a second mint no release answers strands the resume value
# per park. `ora-dyn-sig` is what makes the park a primitive one — a non-literal
# first argument falls through to the runtime primitive, where the literal form
# compiles to the `Emit` terminator whose resume block mints in bytecode. That
# literal form is the control the three witnesses are read against: the same
# program, the same delivery, one path funded by the compiler instead.
(def ora-dyn-sig :yield)
(defn pr-bind [j]
  (let [f (fiber/new (fn []
                       (let [r (emit ora-dyn-sig 7)]
                         [:resumed r])) |:yield|)]
    (fiber/resume f)
    (fiber/resume f (string "b" j))))
(defn pr-tail [j]
  (let [f (fiber/new (fn [] (emit ora-dyn-sig 7)) |:yield|)]
    (fiber/resume f)
    (fiber/resume f (string "t" j))))
(defn pr-keep [j]
  (let [f (fiber/new (fn []
                       (let [r (emit ora-dyn-sig 0)]
                         (emit ora-dyn-sig (length r))
                         (first r))) |:yield|)]
    (fiber/resume f)
    (fiber/resume f (string "k" j))
    (fiber/resume f)))
(defn pr-literal [j]
  (let [f (fiber/new (fn []
                       (let [r (emit :yield 7)]
                         [:resumed r])) |:yield|)]
    (fiber/resume f)
    (fiber/resume f (string "l" j))))
# The `propagate-*` raisers. `fiber/propagate` installs the child's parked
# payload as the propagating fiber's own `signal`, which is a FRESH park and owes
# its own delivery reference — the one the propagating fiber's resumer consumes
# when it releases its resume result (docs/impl/region/owner.md § "Park/unpark
# symmetry"). `defer` is that propagate in production form: it resumes a body
# fiber, runs cleanup, then propagates when the body did not complete. So the
# rate is read across propagate DEPTH: a mint no release answers strands one
# region per park, which makes growth scale with the number of `defer`s the raise
# passes through, and `propagate-none` is the same raise with no propagate in it
# at all.
(defn pg-raise [n]
  (error {:reason :bang :tag (string "z" n)}))
(defn pg-none [j]
  (let [[ok? err] (protect (pg-raise j))]
    (length err:tag)))
(defn pg-one [j]
  (let [[ok? err] (protect (defer
                             nil
                             (pg-raise j)))]
    (length err:tag)))
(defn pg-three [j]
  (let [[ok? err] (protect (defer
                             nil
                             (defer
                               nil
                               (defer
                                 nil
                                 (pg-raise j)))))]
    (length err:tag)))
# The `env-cell-def-capture` / `env-cell-let-twin` pair drives a captured `def`
# inside a lambda, whose init is a CALL (a constant folds and allocates nothing to
# strand). Such a binding is env-celled, so its init's release is routed through
# the env index rather than the stack slot of the same number, and the pair splits
# on whether a cell exists at all: `def` is always mutable, hence always celled,
# while the `let` twin's immutable local is captured by value and never gets one.
# So the gap between them isolates the env route rather than the shape, and a
# release that never reaches the env strands the init on every execution.
(defn ec-increment [x]
  (+ x 1))
(defn ec-def-capture []
  (let [root "/x"]
    ((fn []
       (def joined (path/join root "a"))
       (let [reader (fn [] (list (string? joined) (ec-increment 1)))]
         (reader))))))
(defn ec-let-twin []
  (let [root "/x"]
    ((fn []
       (let [joined (path/join root "a")]
         (let [reader (fn [] (list (string? joined) (ec-increment 1)))]
           (reader)))))))
# `module-cell-read-window` is the closure-as-module: a lambda whose captured
# `def`s the returned struct's accessors read, and whose last form is that struct
# literal — a NATIVE tail call, so the block after the `TailCall` is reached and
# everything the lowerer put there runs. The captured binding's value and its env
# cell are two REGIONS and the frame-exit relocation answers per region, so the
# pair can be split: an `Immediate` native's result is named by no binding, so the
# frame-held admission refuses it for want of a holder and its release stays
# behind the call, while the cell is admitted on its binding's verdict and moves
# ahead. A move that crosses a read through the cell it frees is declined
# (docs/impl/region/mechanism.md § "A move that crosses a read through the cell it
# frees is declined"), and declining leaves the box release on the closure path —
# the bounded fallback whose cost this gauges. `ptr/from-int` is the `Immediate`
# init that splits the pair; `module-cell-heap-init` is the same module with a
# HEAP init, where the value region has a holder and both releases are admitted
# together, so the gap between them isolates the decline from the module shape.
(defn mod-cell-immediate []
  (def a (ptr/from-int 7))
  (def p (fn [] a))
  {:p p})
(defn mod-cell-heap []
  (def a (string "cap"))
  (def p (fn [] a))
  {:p p})
# The `fresh-env-cell` / `shared-env-cell` pair drives one env cell each, split on
# where the cell is minted. `c` is a captured, REASSIGNED local, so `populate_env`
# mints its cell box once per activation — a fresh region per call — and the frame
# ends in a closure tail call, which puts the box's `DecrefCellRegion` in the dead
# post-`TailCall` block. Relocating it there is the frame-held admission's
# business, and the holder's mutation does not refuse it: the release names the
# BOX, which no `assign` repoints (docs/impl/region/mechanism.md § "A mutated
# holder poisons its value route, not its cell box"). `shared-env-cell`'s cell is
# module-level, minted once for the file, so it measures the same closure call with
# no per-op box at all.
(defn t20-make-cell []
  (def @c 0)
  (let [f (fn []
            (assign c (%add c 1))
            c)]
    (f)))
# `env-cell-read-arm` drives the same box where a branch SIBLING arm reads the
# cell's binding. The capture-use of `c` resolves through `f`'s last use, so the
# box's `decref_point` follows the call into the arm that makes it, and the reading
# arm takes compensation's TAIL release — after its own read, where the head release
# would free the box under that read (docs/impl/region/mechanism.md § "A
# compensating release of an env cell names the box, not the holder's slot"). Driven
# through the reading arm, the only one whose release is new.
(defn t20-read-arm [t]
  (def @c 0)
  (let [f (fn [] c)]
    (if t c (f))))
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
# The window's live-in premise is about the ALLOCATION: that is what "born in an
# arm" means, and what the release's route follows, since `region_to_slot` is
# keyed on a region's allocation site. A binding the arm introduces to ALIAS a
# live-in value records no slot of its own and so decides nothing. Driven through
# the arm that takes no alias, the one whose release is new.
(defn t22-arm-alias-inside [v t]
  (match t
    :a (length v)
    _ (let [w v]
        (length w))))
# The window's iterative boundary is the loop's BODY, not the loop's own node. A
# read of a loop-external binding is anchored at the loop NODE, and the lowerer
# emits a node's releases after it, so that release already runs once per execution
# of the loop — the count the merge label is reached with. Driven through the arm
# that does NOT loop, the one whose release is new.
# The sequence reads are read-only trait dispatchers and declare `Opaque`, so
# they seed nothing on escape's store facet (docs/impl/region/effects.md
# § `Opaque`). A `Mixed` declaration would, and every mechanism gated on
# `frame_held_regions` refuses a region escaping by a facet other than
# return — the branch-arm window among them, which is what this drives.
(defn t22-arm-seq-read [v t]
  (match t
    :a (first v)
    :b (length v)
    _ (length v)))
(defn t22-arm-loop-read [v t]
  (match t
    :a (length v)
    _
      (begin
        (var i 0)
        (while (%lt i 3)
          (get v i)
          (assign i (%add i 1)))
        (%add i 100))))
# The same window over a branch one of whose arms leaves through a frame-replacing
# CLOSURE tail call naming the same parameter — the `append`/`concat` dispatch
# shape. Anchoring is what covers the arm driven here; the frame-exiting arm is
# covered by the relocation's exemption, since its call took the argument over
# (docs/impl/region/mechanism.md § "An arm that leaves through a callee takes a
# replica, not the anchor").
(defn t22-tc-callee [v]
  (length v))
(defn t22-tailcall-sibling [v t]
  (match t
    :a (length v)
    :b (t22-tc-callee v)
    _ 0))
# The same window over a RETURNED parameter — `push-all`'s shape, and with it every
# `append`/`concat` over a byte-family source. The arm driven here hands `dst` back
# to the caller; the sibling arm leaves through a local walker that reaches `dst`
# only through its captured environment. The merge follows this arm's own mint, and
# the sibling's replica runs ahead of a callee whose captured edge holds the region
# off zero until its own mint, so the branch is admitted for the class
# (docs/impl/region/mechanism.md § "The return facet costs the merge nothing").
# Refusing the facet outright strands one whole accumulator per call on the arm
# driven here.
(defn t22-returned-captured [dst src]
  (if (%eq (type-of src) :string)
    (begin
      (push dst src)
      dst)
    (let [n (length src)]
      (letrec [go (fn (i)
                    (if (%lt i n)
                      (begin
                        (push dst (get src i))
                        (go (%add i 1)))
                      dst))]
        (go 0)))))
# The frame-exit release (docs/impl/region/mechanism.md § "A release past a
# frame-replacing tail call is not a release"). `t23-unused`'s parameter is used
# nowhere, so its release is the unused-parameter fallback the lowerer emits at the
# end of the body — the block a CLOSURE callee never reaches — and escape clears it
# as frame-held. `t23-moved` is the exemption: its parameter IS the tail call's
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
# `t23-captured`'s parameter is reached by the tail callee through its CAPTURED
# environment — the path no argument names and no callee region describes. It is
# admitted all the same: the funnel counted the closure's hold when the env was
# built, so the relocated release drops the frame's own reference and leaves the
# callee's standing (docs/impl/region/mechanism.md § "Lexical capture is not a
# second holder to fear").
(defn t23-captured [x]
  (let [g (fn [] (length x))]
    (g)))
# `t23-handback` is the same capture carrying one step further: the tail callee
# RETURNS the parameter, so the caller's owning reference is minted inside the
# callee — after the relocated release has run. The env's counted edge is what
# holds the region off zero in between, and it falls away only with the closure
# region, at the callee's completion (docs/impl/region/mechanism.md § "The callee's
# return mint, and why the point owes it nothing"). This is the stdlib `push-all`
# shape, and the strand it carried is what held the `concat`/`append` family.
(defn t23-handback [dst src]
  (let [n (length src)]
    (letrec [go (fn [i]
                  (if (%lt i n)
                    (begin
                      (push dst (get src i))
                      (go (%add i 1)))
                    dst))]
      (go 0))))
(defn t23-drive-handback [src]
  (let [acc (@array)]
    (t23-handback acc src)
    (length acc)))
# `t23-fwd-cell`'s `go` reaches its sibling `helper` through a prebound FORWARD
# CELL, and `helper` does not call back — a one-way sibling capture, so there is no
# SCC and the closure-cycle merge never sees the cell. Its binding-scope
# `DecrefRegion` lands in the dead block like any other of the frame's releases, and
# the count argument reaches it through its BINDING's verdict: a binding names the
# closure region its cell points at, never the cell's own, so an admission read over
# holder bindings alone cannot see the cell at all. Stranding the cell strands the
# sibling with it, the cell's reference being what holds that closure off zero
# (docs/impl/region/mechanism.md § "A compiled capture cell is frame-held exactly as
# its binding is"). `t23-fwd-cell-ret` is the RETURN face of the same projection:
# the capturer is handed back, so what keeps the cell alive across the relocated
# release is the counted `closure ⊇ cell` edge, and the cell's own region has to
# carry its binding's verdict for the projection to name it at all.
(defn t23-fwd-cell [n]
  (letrec [helper (fn [x] (%sub x 1))
           go (fn [m] (helper m))]
    (go n)))
(defn t23-fwd-cell-ret [n]
  (letrec [helper (fn [x]
                    (when (%not (%int? x)) (error :x))
                    (%sub x 1))
           go (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) go (go (helper m))))]
    (go n)))
# `t23-fwd-cell-sib` inverts `t23-fwd-cell`: the SIBLING captures the self-recursive
# member, and the body tail-calls the sibling. That one `TailCall` strands two
# regions on two independent channels — the merged arena's `deferred_release_slot`
# (`go`'s closure, its env, and the forward cell the single-closure self-edge
# admission collapsed into it) and the sibling's own `defer_callee_release`. RED if
# the runtime reads them as alternatives again, which reclaims neither: the
# sibling's counted `closure ⊇ cell` edge holds the arena off zero until the
# sibling's own region goes (docs/impl/region/letrec.md § "The arena channel and the
# callee channel are independent").
(defn t23-fwd-cell-sib [n]
  (letrec [go (fn [m] (if (%lt m 1) :done (go (%sub m 1))))
           outer (fn [m] (go m))]
    (outer n)))
# `t23-operand-value`'s tail call names `go` NOWHERE — its ARGUMENT calls `go`, so
# what the callee is handed is that call's result and `go`'s own closure region was
# read and finished with before the call was made. The exemption reads an operand's
# VALUE rather than its syntax, so the region an argument's own nested call merely
# used is not exempt and its scope-end release relocates like any other
# (docs/impl/region/mechanism.md § "What an operand names is its VALUE, not its
# syntax"). RED if the exemption widens back to the syntax walk, which strands one
# closure per call for every stdlib helper whose tail call consumes a local walker's
# result.
(defn t23-operand-value [n]
  (letrec [helper (fn [x] (%sub x 1))
           go (fn [m] (if (%lt m 1) 0 (go (%sub m 1))))]
    (helper (go n))))
# `t23-callee-member` swaps `t23-operand-value`'s capturer for a plain one, so the
# body tail-calls the CAPTURED sibling: `helper` is allocated per call and its uses
# span the letrec, which puts its demise at the letrec's scope end rather than at
# the call node. Its own region is exempt from the relocation because the new
# activation takes the release over, so the deferral has to reach a release placed
# at that scope end and not only one demising at the call node
# (docs/impl/region/mechanism.md § "What the exemption keeps, a channel must still
# run"). RED if the deferral narrows back to the call node, which strands one
# closure plus its environment per call for every helper pair whose body dispatches
# the captured member.
(defn t23-callee-member [n]
  (letrec [helper (fn [x] (%sub x 1))
           go (fn [m] (helper m))]
    (helper (go n))))
# `t23-fold-drive` is the index-walk fold driver every stdlib `fold`/`reduce`/
# `concat` walks with. Its base arm returns the accumulator, so the region is on the
# return frontier; its recursive arm hands the tail callee the COMBINER's result
# rather than the accumulator itself, so no route reaches the accumulator at that
# point. A callee reaches a value this frame owns as an operand or through its
# captured environment and by no other route, so one it reaches by neither cannot
# mint against the region and the relocated release is the last
# (docs/impl/region/mechanism.md § "The callee's return mint, and why the point owes
# it nothing"). RED if the admission narrows back to a per-point funding edge, which
# strands one displaced accumulator per fold step.
(defn t23-fold-step [f n i acc]
  (if (%lt i n) (t23-fold-step f n (%add i 1) (f acc i)) acc))
(defn t23-fold-drive [n]
  (length (t23-fold-step (fn [a b] (@array)) n 0 (@array))))
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
# A signal the compiler cannot read as a keyword set, and a payload no fiber body
# allocates — the two ingredients a dynamic emit's borrowed park needs.
(def emit-sig :yield)
(def emit-error-sig :error)
(def emit-subject (string "emit-subject"))

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
        (fiber/resume f))) 0]  # The fiber value installers (`fiber/resume`/`abort`/`cancel`/`emit`) declare
   # `Delivers`: the install into another fiber's signal slot counts its own
   # reference at runtime, so no arg clique. `Mixed` charged one never-balancing
   # `IncrefRegion` per heap-argument pair, which a HEAP payload is what arms —
   # an immediate one leaves the pair unformed and measures nothing either way.
   # The control for the class (docs/impl/region/effects.md § `Delivers`); the
   # four installers' inline faces are pinned by
   # tests/elle/region-fiber-install-clique-leak.lisp.
   ["fiber-deliver"
    (fn [j]
      (let [f (fiber/new (fn []
                           (yield 1)
                           2) |:yield|)]
        (fiber/resume f)
        (fiber/resume f [j]))) 0]
   ["array-literal" (fn [j] [j (+ j 1) (+ j 2)]) 0] ["mut-array" (fn [j] @[]) 0]
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
   # zip's F1a copy-scratch is dissolved: its column walks are cell-free top-level
   # drivers threading `arrs`/`out` and their accumulators as params
   # (`zip-tuple-at`/`zip-build-array`/`zip-build-list`), so the walks allocate no
   # closure and no capture cell. What remained was the per-path return frontier
   # (docs/impl/region/mechanism.md § "The return frontier is per-path"): every one
   # of those drivers is a walk whose base case
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
   # `distinct` is a CLOSED control for the arm reading (undeclared, like
   # `rest-array-copy`), so a regression to open trips the completeness gate loudly
   # rather than being absorbed as F1a scratch. Its dispatch is a `cond` naming
   # `coll` in every clause TEST, and a clause test is a conditional position
   # exactly as a clause body is (docs/impl/region/mechanism.md § "An arm is a
   # conditional position, not a syntactic arm body"), so the argument's one
   # release sat in the LAST test and every call taking an earlier body stranded
   # the whole input.
   ["distinct" (fn [j] (distinct [1 2 1 3])) 0]
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
   # `group-by`/`frequencies`/`merge`/`each-list` are CLOSED controls for the
   # release ROUTE reading (undeclared, like `rest-array-copy`): one binding owns a
   # region's route — the one whose init allocated it — so a second name bound from
   # the value refuses nothing (docs/impl/region/mechanism.md § "A mutated holder
   # poisons its value route, not its cell box"). Each of these walks its input with
   # a reassigned cursor bound inside a type-dispatch arm, and reading the mutation
   # off the cursor held the whole input per call. A regression to open must trip the
   # completeness gate loudly rather than be absorbed back into F1a.
   ["group-by" (fn [j] (group-by odd? [1 2 3 4])) 0]
   ["frequencies" (fn [j] (frequencies [1 2 1 3])) 0]
   ["to-array" (fn [j] (->array (list 1 2 3))) 0]
   ["to-list" (fn [j] (->list [1 2 3])) 0]
   ["freeze" (fn [j] (freeze @[1 2 3])) 0]
   ["slice" (fn [j] (slice [1 2 3 4] 1 3)) 0]  # trailing nil keeps the body's value a discarded STATEMENT, matching the
   # original while-loop (where the alloc is never the loop's tail value)
   ["keys-values"
    (fn [j]
      (keys {:a 1 :b 2})
      (values {:a 1 :b 2})
      nil) 0] ["merge" (fn [j] (merge {:a 1} {:b 2})) 0]
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
   # `concat` folds the extra arguments through `core-fold-step`, whose accumulator
   # is a returned parameter the recursive arm hands its callee only through the
   # combiner's RESULT. That point cannot reach the accumulator, so it owes no
   # funding edge and each displaced one is freed per step
   # (docs/impl/region/mechanism.md § "The callee's return mint, and why the point
   # owes it nothing"). The 2-argument shape is `stdlib-concat` below; this is the
   # 3-argument one, where the fold actually recurses.
   ["concat" (fn [j] (concat "a" "b" "c")) 0]
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
          (assign k (%add k 1))))) 0]  # A CURSOR walked over a cons chain, whose cell's INIT carries a second
   # name: `xs` holds the chain head for the whole call. A cell donates its init
   # only where it is that value's sole holder, so the alias costs the donation —
   # and the cell counts the init instead, keeping the container model and the
   # STORE-SITE PIN it carries (docs/impl/region/bindings.md § "What the cell
   # donates it must hold alone; what it counts it need not"). Refusing the model
   # instead rides each step's release out to the cell's last use, so one release
   # covers the whole walk and every cons the cursor passed stays live.
   #
   # `each-manual` directly above is the same `while` loop over an ARRAY and
   # reads 0 either way — an index walk holds no cell — so the gap between the
   # two isolates the cell rather than the loop.
   ["list-cursor"
    (fn [j]
      (let [xs (list 1 2 3 4)
            @r xs
            @n 0]
        (while (not (empty? r))
          (assign n (%add n 1))
          (assign r (rest r)))
        n)) 0]  # The same walk with the alias taken AFTER the cell binding, so the CELL's own
   # binder is what allocated the init and the counted-init route has no untainted
   # slot to release the producer's reference through — the only one recorded for
   # the init region is the reassigned cell's, and no release may route through a
   # mutated slot. What reclaims it is the reader instead: `keep` is a whole-value
   # read of a 1-slot container, so it takes a COUNTED reference of its own,
   # released through its own never-repointed slot, which withdraws it from the
   # sole-held question and hands the donation back to the cell
   # (docs/impl/region/bindings.md § "A whole-value read of a 1-slot container
   # takes a counted reference"). `list-cursor` directly above is the same walk
   # with the alias taken BEFORE, where the alias allocates and the counted-INIT
   # route runs, so the gap between the two isolates the route rather than the
   # model.
   ["cell-alias-after"
    (fn [j]
      (let [@r (list 1 2 3 4)]
        (let [keep r]
          (def @n 0)
          (while (not (empty? r))
            (assign n (%add n 1))
            (assign r (rest r)))
          (list n (first keep))))) 0]
   # A branch inside a loop stores into ONE cell from both arms, so the cell has
   # two store sites and each arm allocates the value it stores. Each stored
   # value's producer release is discharged at the store that took THAT value
   # (docs/impl/region/bindings.md § "The store site is the store that took THAT
   # value"). Pinning both values at the cell's LAST store puts the first arm's
   # release inside the second arm, which that arm's path does not reach, so an
   # iteration repeating the first arm displaces the previous value from its own
   # ANF slot with nothing left to release it. `list-cursor` above is the
   # single-store control — one arm, one site, nothing to mis-pair.
   ["cell-arm-store"
    (fn [j]
      (let [@last (array 0 0)]
        (def @i 0)
        (while (%lt i 4)
          (if (%lt (%mod i 4) 2)
            (assign last (array i 7))
            (assign last (array i 9)))
          (assign i (%add i 1)))
        (get last 1))) 0] ["format" (fn [j] (string "iter " j " of " 100)) 0]
   # The four-stage `split`/`map`/`filter`/`join` chain — a CLOSED control now
   # (undeclared, like `rest-array-copy`), so a regression to open trips the
   # completeness gate rather than being absorbed as F1a scratch. Every stage
   # dispatches through a `cond` over its argument's type, so each call stranded
   # its whole input on the clause-test reading above.
   ["pipeline"
    (fn [j]
      (string/join (filter (fn [x] (not= x ""))
                           (map string/trim (string/split "a , b , c" ","))) ","))
    0]
   ["each-list"
    (fn [j]
      (each x in (list 1 2 3)
        {:val x})) 0]
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
   # A CLOSED control for the call-result naming rule (undeclared, like
   # `rest-array-copy`), so a regression to open trips the completeness gate
   # loudly instead of being absorbed as F1a scratch. The walk inlines `f`'s
   # body, and the regions that walk yields — here the one the inner lambda's
   # closure+env live in — name the CALLEE's activation; the caller's temp holds
   # the call's own region instead (docs/impl/region/mechanism.md § "A call's
   # result is named by the call's own region"). Adopting them made the caller a
   # second, nominal holder and dragged the region's one release onto a node the
   # allocating path never reaches.
   ["nested-closure"
    (fn [j]
      (let [f (fn [] (fn [] j))]
        ((f)))) 0] ["user-struct" (fn [j] (make-struct j)) 0]
   ["user-string" (fn [j] (make-label j)) 0] ["chain" (fn [j] (process j)) 0]
   # The gauge for the UN-fused stdlib `map`: the kernel CAPTURES `k`, a shape loop
   # fusion declines (splicing a capture at the call site is out of scope), so the
   # real `map` runs and its per-call strands are measured. A CLOSED control now
   # (undeclared, like `rest-array-copy`), so a regression to open trips the
   # completeness gate rather than being absorbed as F1a scratch: `map` dispatches
   # on its collection's type through a `cond`, whose later clause TESTS held the
   # argument's one release (docs/impl/region/mechanism.md § "An arm is a
   # conditional position, not a syntactic arm body"). Its `push-accum` face is the
   # same op driven into a block-local accumulator.
   ["wrap-map"
    (fn [j]
      (let [k 1]
        (map (fn [x]
               (numeric!)
               (%add x k)) [1 2 3]))) 0] ["factory" (fn [j] (t13proc j)) 0]
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
   ["fresh-env-cell" (fn [j] (t20-make-cell)) 0]
   ["env-cell-read-arm" (fn [j] (t20-read-arm true)) 0]
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
   ["concat-while" (fn [j] (concat "x" (number->string j))) 0]
   ["protect-while"
    (fn [j]
      (let [[ok v] (protect ((fn [] j)))]
        v)) 0]  # `defer` on its ordinary SUCCESS path — the twin of `protect-while`
   # above. Same inner fiber, same resume; the whole difference is the trailing
   # `if`, which reads the fiber with `fiber/value` in the arm taken here and with
   # `fiber/propagate` in the arm that is not. Declaring `fiber/propagate` `Mixed`
   # seeds that fiber on escape's store facet, the branch-arm release window
   # declines, and the branch's only release stays in the untaken arm — 2 regions
   # and 3 objects stranded per evaluation, so a loop whose body is wrapped in
   # `defer` grows without bound (docs/impl/region/effects.md § `Opaque`, "The
   # child-chain WIRING is `Opaque` too"). `protect-while` has no such arm and reads
   # 0 whatever the declaration says, which is what makes it the pair-control here
   # and not the gauge. Both are CLOSED controls (undeclared, like
   # `rest-array-copy`), so a regression to open trips the completeness gate loudly
   # instead of being absorbed under F2.
   ["defer-while"
    (fn [j]
      (defer
        (length [1 2])
        ((fn [] j)))) 0]  # The other arm, and the control.
   # `defer-error` raises in the body, so the arm it drives is the PROPAGATE arm —
   # the one that held the branch's only release under `Mixed`, and so the one that
   # correctly stays closed under `defer-while`'s counterfactual. It is what tells a
   # real fix from a moved strand: a change that only re-anchored the release onto
   # the other arm closes `defer-while` and opens this. The arm also leaves by
   # SIG_PROPAGATE rather than falling through, so a release the window replicates
   # into it must survive a signal exit to read 0 here.
   ["defer-error"
    (fn [j]
      (protect (defer
                 (length [1 2])
                 (error j)))) 0]
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
        (get (fiber/value f) :error))) 2]  # A parked fiber hard-killed by `fiber/cancel` reclaims fully: the kill
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
        (fiber/status f))) 0]  # `fiber/abort` of a PARKED fiber. `fiber/abort` is a
   # native tail call here, and its fiber argument is a captured upvalue — a BORROWED
   # tail argument, for which the frame mints a fresh owning reference so the callee
   # has one to release. The abort leaves by SIG_ABORT, which reaches neither consumer
   # of that retain (a frame-replacing closure callee's owned-param release, or the
   # post-`TailCall` fall-through the native's normal completion runs), so the signal
   # exit consumes it itself (docs/impl/region/mechanism.md § "What the fall-through
   # owes, a signal exit owes too"). The stranded reference was the fiber's own, which
   # pinned the body closure and the parked frame's payload behind it. A CLOSED control
   # now, beside `denied-discard`, whose residual is the DENIED call's own argument
   # scratch and stays open under F2.
   ["abort-discard"
    (fn [j]
      (let [f (fiber/new (fn []
                           (yield j)
                           9) |:yield|)]
        (fiber/resume f)
        (protect (fiber/abort f "boom")))) 0]  # An abandoned park through the DYNAMIC
   # emit path: a first argument the compiler cannot read as a keyword set falls
   # through to the `emit` primitive, so the park is an ordinary call rather than the
   # `Emit` terminator and the body reference the discharge stands in for comes from
   # the call rather than from `lower_emit` (docs/impl/region/owner.md § "What yields
   # is the emit OPERATION, not the `Emit` node"). Each gauges the reference's ARITY,
   # not its presence — withholding it over-frees, which no leak gauge sees and
   # `tests/elle/region-dynamic-emit-borrow-uaf.lisp` reports. The four must stay
   # together: the two witnesses differ only in POSITION, which decides where the
   # reference comes from — a non-tail park mints one at the payload argument, a tail
   # park already has the borrowed-argument retain and the suspending exit leaves it
   # standing (docs/impl/region/mechanism.md § "What the fall-through owes, a signal
   # exit owes too") — and each has a control that removes one ingredient:
   # `emit-lit-discard` takes the literal path with the same borrow, and
   # `emit-dyn-fresh` takes the dynamic path with a payload the body allocates, where
   # nothing is owed and a mint would strand one per park. CLOSED controls
   # (undeclared, like `rest-array-copy`), so a regression to open trips the
   # completeness gate loudly rather than being absorbed under F2.
   ["emit-dyn-discard"
    (fn [j]
      (let [f (fiber/new (fn []
                           (emit emit-sig emit-subject)
                           9) |:yield|)]
        (fiber/resume f))) 0]
   ["emit-dyn-tail"
    (fn [j]
      (let [f (fiber/new (fn [] (emit emit-sig emit-subject)) |:yield|)]
        (fiber/resume f))) 0]
   ["emit-lit-discard"
    (fn [j]
      (let [f (fiber/new (fn []
                           (emit :yield emit-subject)
                           9) |:yield|)]
        (fiber/resume f))) 0]
   ["emit-dyn-fresh"
    (fn [j]
      (let [f (fiber/new (fn []
                           (emit emit-sig (string "v" j))
                           9) |:yield|)]
        (fiber/resume f))) 0]  # The same operation raising a TERMINAL signal, where
   # the reference the tail call holds answers to a different consumer: the payload's
   # DELIVERY, released by whoever catches the signal. The exit consumes its
   # borrowed-argument retains — the block that would have consumed them is abandoned,
   # and an `:error` fiber's restart replays it — so it mints the delivery and records
   # it, the pair `handle_emit` performs on the literal path
   # (docs/impl/region/mechanism.md § "What the fall-through owes, a signal exit owes
   # too"). CLOSED controls (undeclared, like `rest-array-copy`), so a regression to
   # open trips the completeness gate loudly rather than being absorbed under F2.
   # Each reads the mint's ARITY: withholding it over-frees, which no leak gauge sees
   # and tests/elle/region-dynamic-emit-terminal-uaf.lisp reports. The six must stay
   # together, because only the gaps between them separate the mint from the record.
   # `emit-dyn-error-fresh` allocates its payload in the body, so the frame's own
   # reference is what the record reclaims and a mint per reference reads 1;
   # `emit-dyn-error-repeat` names one region through BOTH arguments, so the frame
   # holds a moved reference and a retain and the walk must run the release the
   # exemption used to skip; `emit-dyn-error-restart` resumes the raised fiber, so the
   # replayed block releases the frame's reference where the discharge otherwise
   # would. `emit-dyn-error-discard` and `emit-lit-tail-error` remove one ingredient
   # each — the tail position, and the primitive itself.
   ["emit-dyn-tail-error"
    (fn [j]
      (let [f (fiber/new (fn [] (emit emit-error-sig emit-subject)) |:error|)]
        (fiber/resume f))) 0]
   ["emit-dyn-error-discard"
    (fn [j]
      (let [f (fiber/new (fn []
                           (emit emit-error-sig emit-subject)
                           9) |:error|)]
        (fiber/resume f))) 0]
   ["emit-lit-tail-error"
    (fn [j]
      (let [f (fiber/new (fn [] (emit :error emit-subject)) |:error|)]
        (fiber/resume f))) 0]
   ["emit-dyn-error-fresh"
    (fn [j]
      (let [f (fiber/new (fn [] (emit emit-error-sig (string "v" j))) |:error|)]
        (fiber/resume f))) 0]
   ["emit-dyn-error-repeat"
    (fn [j]
      (let [f (fiber/new (fn []
                           (let [t (set :error)]
                             (emit t t))) |:error|)]
        (fiber/resume f))) 0]
   ["emit-dyn-error-restart"
    (fn [j]
      (let [f (fiber/new (fn [] (emit emit-error-sig (string "v" j))) |:error|)]
        (fiber/resume f)
        (fiber/resume f))) 0]  # An emit-raised error's payload keeps
   # every frame-owed release: `(error v)` mints the payload's delivery itself (the
   # `EmitEscape` retain the resumer's release of the resume result consumes), so the
   # raise records the mint and the abandoned-frame walk and the parked frame's
   # discharge stop exempting the payload's region (docs/impl/region/mechanism.md
   # § "An abandoned frame runs the releases it still owes"). CLOSED controls
   # (undeclared, like `rest-array-copy`), so a regression to open trips the
   # completeness gate loudly rather than being absorbed under F2; the soundness
   # complement is tests/elle/region-error-payload-uaf.lisp. The faces are distinct
   # consumers of the recorded mint and must stay together: `error-payload` raises
   # in the try's own body frame, which is PARKED for the restarts system, so its
   # release runs at the free-path discharge; `error-payload-helper` raises in a
   # called frame, which is WALKED at the error exit; `error-payload-param` hands
   # the payload down as an owned parameter, so the tail-replaced parked frame owes
   # it through the prologue-recorded slot; `error-payload-struct` raises a struct whose message
   # string is a second region, so both of the frame's tables are owed.
   # `error-payload-native` is the pair-control for all four: a native raise
   # installs its payload unretained, so the frame-funded exemption stays — the gap
   # isolates the recorded mint from the walk and discharge themselves.
   # `error-payload-helper` calls its raiser as a STATEMENT: a bare call in the
   # try body is a frame-replacing tail call, which lands in the parked frame like
   # `error-payload` — only the non-tail call leaves a callee frame for the walk.
   # Its pin is CROSS-TIER at 0: both tiers walk the abandoned frame, the compiled
   # one off the tables its prologue materialized and the locals it spills at the
   # exit, so the rate agrees under --jit=off and --jit=eager. The compiled face
   # has its own gauge in tests/elle/region-jit-error-unwind.lisp.
   ["error-payload"
    (fn [j]
      (try
        (error (string "x" j))
        (catch e nil))) 0]
   ["error-payload-helper"
    (fn [j]
      (try
        (begin
          (ep-raiser j)
          nil)
        (catch e nil))) 0]
   ["error-payload-param"
    (fn [j]
      (try
        (ep-raise-param (string "x" j))
        (catch e nil))) 0]
   ["error-payload-struct"
    (fn [j]
      (try
        (error {:error :e :message (string "m" j)})
        (catch e nil))) 0]
   ["error-payload-native"
    (fn [j]
      (try
        (get j :k)
        (catch e nil))) 0]  # The two fiber-crossing DELIVERIES, both CLOSED
   # controls (undeclared, like `rest-array-copy`) so a regression to open trips
   # the completeness gate loudly rather than being absorbed under F2. Each
   # gauges a mint's ARITY rather than its presence: withhold the mint and the
   # crossing over-frees, which is a soundness failure no leak gauge can see and
   # `--trace=guardfree` reports (`region_primitive_resume_uaf`,
   # `region_fiber_propagate_uaf` in tests/integration/elle_scripts.rs); mint
   # more than one and the surplus is a reference no release answers, which is
   # what these rates catch.
   #
   # `primitive-resume-*` are the three shapes a parked primitive's resume value
   # reaches — bound by the body, returned from tail position, and held across a
   # further park — beside `emit-resume-literal`, the control whose resume block
   # mints in bytecode and so is correct with no delivery mint at all. A parked
   # CAPABILITY DENIAL is the fourth park of this shape and has no probe here on
   # purpose: its rate is dominated by the denied call's own argument scratch,
   # which `denied-discard` already declares under F2, so a probe there would
   # count one root twice instead of gauging the delivery. Its soundness face is
   # the `w-denied` witness under guardfree.
   ["primitive-resume-bind" (fn [j] (pr-bind j)) 0]
   ["primitive-resume-tail" (fn [j] (pr-tail j)) 0]
   ["primitive-resume-keep" (fn [j] (pr-keep j)) 0]
   ["emit-resume-literal" (fn [j] (pr-literal j)) 0]
   # `propagate-*` read the same mint across propagate DEPTH: the three must stay
   # together, because a surplus delivery reference strands one region per park
   # and only the depth gap tells that apart from the raise's own cost.
   # `propagate-none` is that raise with no propagate in it, and it is a scalar 0
   # rather than the baseline a differential would subtract — the raising body's
   # own reference to the payload it allocated is released by the abandoned
   # frame's release-table walk (§ `error-payload*` above), so there is nothing
   # left for a depth difference to hide behind.
   ["propagate-none" (fn [j] (pg-none j)) 0]
   ["propagate-one" (fn [j] (pg-one j)) 0]
   ["propagate-three" (fn [j] (pg-three j)) 0]
   # The captured-`def` env cell and its `let` twin, CLOSED controls for the env
   # ROUTE. Undeclared, like `rest-array-copy`.
   ["env-cell-def-capture" (fn [j] (ec-def-capture)) 0]
   ["env-cell-let-twin" (fn [j] (ec-let-twin)) 0]
   # The closure-as-module pair, CLOSED controls for the declined move. The
   # `Immediate` init is what splits the binding's two regions; the heap init is
   # the same module with both admitted together.
   ["module-cell-read-window" (fn [j] (mod-cell-immediate)) 0]
   ["module-cell-heap-init" (fn [j] (mod-cell-heap)) 0]])

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

# RETURNED closure cycle — the return-funded merge admission (rate 0). The same ev/od
# SCC as `recur-local-mutual` above, one base case apart: it returns the MEMBER `ev`
# instead of a keyword, putting a member on the return frontier. The merge admits it
# anyway, because the merge's release is a decref rather than a free and the returned
# member lives IN the merged arena, so the callee's `Return` mint raises the arena's own
# count — and the letrec body's tail is a call to the MEMBER `ev`, whose deferral runs at
# the recursion's normal completion, AFTER that mint. So the deferral drops only the
# frame's reference while the caller's stands, and the discard at the call site takes the
# arena to zero and subtree-drops the cycle (docs/impl/region/letrec.md § The frontier
# gate). The FIBER half of the frontier still refuses outright, and a returned cycle
# whose letrec body does not hand the value over itself keeps the Shared baseline — its
# binding-scope drop would then fire before any mint. Refusing the whole return facet
# instead holds this cycle's four regions — two closures, two forward cells — per call.
(defn lcl-mutual-ret [n]
  # `ev` is returned (a value use), which disables call-site param joins, so a local
  # diverging guard proves the %lt/%sub operands.
  (letrec [ev (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) ev (od (%sub m 1))))
           od (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) ev (ev (%sub m 1))))]
    (ev n)))
(pin (measure "recur-local-mutual-ret" (fn [j] (lcl-mutual-ret 3)) 100 6 60 0.4
              0.5) 0)

# The same returned ev/od cycle, one body-tail apart: it tail-calls a NON-member
# (`lcl-ident`) rather than the member `ev`. Which channel carries the arena's release
# does not enter the ordering argument, and neither does the fact that the compiler
# cannot classify the callee: a CLOSURE callee replaces the frame and takes the
# `deferred_release_slot` deferral at the recursion's completion, while a NATIVE callee
# keeps the frame and falls through to the binding-scope drop the lowerer emits at the
# `Letrec` node — after the mint the tail call itself emits at the call site. Both are
# after the mint, so the merge admits the return facet here too (rate 0). Refusing it
# held this cycle's four regions — two closures, two forward cells — per call. Undeclared,
# like `rest-array-copy`: a regression to open must trip the completeness gate as an F4
# defect rather than be absorbed under the root.
(defn lcl-ident [x]
  x)
(defn lcl-mutual-ret-foreign [n]
  (letrec [ev (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) ev (od (%sub m 1))))
           od (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) ev (ev (%sub m 1))))]
    (lcl-ident (ev n))))
(pin (measure "recur-local-mutual-ret-foreign"
              (fn [j] (lcl-mutual-ret-foreign 3)) 100 6 60 0.4 0.5) 0)

# The third admitted body shape: a bare member VALUE tail, no tail call at all. The
# letrec is this frame's tail, so functionalization puts the frame's `Return` INSIDE the
# letrec body and it mints there, ahead of the binding-scope drop. A closed control,
# undeclared for the same reason as `recur-local-mutual-ret-foreign` above.
(defn lcl-mutual-ret-value [n]
  (letrec [ev (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) ev (od (%sub m 1))))
           od (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) ev (ev (%sub m 1))))]
    ev))
(pin (measure "recur-local-mutual-ret-value" (fn [j] (lcl-mutual-ret-value 3))
              100 6 60 0.4 0.5) 0)

# The fourth body shape: the identical returned ev/od cycle, one BINDING out of tail
# position. The letrec's value is bound to `c` and handed on by a later statement, so the
# body falls out to a bare value with no `Return` and no tail call of its own — `c` names
# the member's region directly, an uncounted read, and the mint that funds the caller is
# an enclosing node away. Pinning the arena's release at the binding scope would fire it
# at the letrec, under a live `c`; the merge instead follows the value out and adopts the
# release point the last-use rule already computed for the handed-out member — here the
# enclosing `Return`, whose mint precedes that node's own releases
# (docs/impl/region/letrec.md § "Drop site — following a handed-out member"). This is the
# boundary control for `recur-local-mutual-ret-value` above — the two differ only in
# whether the letrec is the frame's tail, which is exactly the fact that decides where the
# mint lands — so the pair reads the two dispositions directly rather than by
# resemblance. Refusing this shape held its four regions — two closures, two forward
# cells — per call.
(defn lcl-mutual-ret-bound [n]
  (let [c (letrec [ev (fn [m]
                        (when (%not (%int? m)) (error :m))
                        (if (%lt m 1) ev (od (%sub m 1))))
                   od (fn [m]
                        (when (%not (%int? m)) (error :m))
                        (if (%lt m 1) ev (ev (%sub m 1))))]
            ev)]
    (lcl-ident n)
    c))
(pin (measure "recur-local-mutual-ret-bound" (fn [j] (lcl-mutual-ret-bound 3))
              100 6 60 0.4 0.5) 0)

# ── Retained-closure reclamation (a RETURNED self-recursive closure's region) ──
# `recur-local-self` above pins the LEAK rate of a self-recursive closure used as a
# LOOP (0 — cell-free, reclaimed per call). These two RETAIN each returned closure in
# a block-local @keep, so the question becomes whether the closure's own region
# reclaims when @keep is freed at the block's return.
#
# `lcl-self-ret`'s `go` is cell-free and self-recursive, so the lowerer strands its
# scope-end `DecrefRegion` past the letrec body's frame-replacing tail call and the
# runtime deferred release is the region's ONLY channel. A returned closure keeps
# that channel: the callee's `Return` mints the caller's reference before
# `trampoline_loop` breaks and runs the deferred decref, so the caller's reference is
# standing while the deferral drops the frame's own (docs/impl/selfrec.md § "The
# deferral needs no escape gate"). The CONTROL
# `lcl-foreign-ret` is not self-recursive, so nothing strands its release in the
# first place and the gap isolates the strand rather than the retain. Object growth,
# not region growth, is the gauge (closure + env share one region). The
# self-recursive LOOP being cell-free is a distinct property, pinned
# deterministically by runtime::tests::ownership::self_recursive_loop_is_cell_free;
# the soundness half — that the returned handle is still live after the deferred
# release — is pinned under the UAF oracle by
# tests/elle/region-selfrec-return-release.lisp.
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
                   0.4 0.5) 0)
(pin (measure-core "recur-local-foreign-mint"
                   (retain-block (fn [] (lcl-foreign-ret 3))) count-gauge 100 6
                   60 0.4 0.5) 0)

# ── The same strand, handed across the FIBER frontier ──────────────────
# `recur-local-self-yield` and `recur-local-self-send` are CLOSED controls
# (undeclared, like `rest-array-copy`) for the fiber half of the stranded-self
# deferred release. Each hands its cell-free self-recursive closure across a fiber
# frontier — emitted to the resumer, or sent over a channel — and then tail-calls it,
# so the scope-end `DecrefRegion` is dead past that `TailCall` and the deferral is the
# region's only channel. The crossing is no reason to withhold it: the emit's park
# retain into `fiber.signal` (which the resumer's result release consumes) and
# `chan/send`'s send-site incref each count a reference of their own, so the deferral
# drops the frame's alone (docs/impl/selfrec.md § "The deferral needs no escape
# gate"). `recur-local-self` above is the control — the same strand with no crossing —
# so the gap between them isolates the crossing rather than the strand. The soundness
# half, that the delivered handle is still live after the deferred release, is pinned
# under the UAF oracle by tests/elle/region-selfrec-fiber-release.lisp.
(defn lcl-self-yield [n]
  "Self-recursive local closure YIELDED to the resumer before the body tail-calls it."
  # go crosses the frontier (a value use), which disables call-site param joins, so a
  # local diverging guard proves the %lt/%sub operands (as in lcl-self-ret).
  (letrec [go (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) :done (go (%sub m 1))))]
    (yield go)
    (go n)))
(def [lcl-snd lcl-rcv] (chan))
(defn lcl-self-send [n]
  "The same closure SENT over a channel — the other fiber-frontier seed."
  (letrec [go (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) :done (go (%sub m 1))))]
    (chan/send lcl-snd go)
    (go n)))
(pin (measure "recur-local-self-yield"
              (fn [j]
                # Two resumes per op: the first runs to the yield, the second runs the
                # recursion, whose normal completion is where the deferral fires.
                (let [f (fiber/new (fn [] (lcl-self-yield 3)) |:yield|)]
                  (fiber/resume f)
                  (fiber/resume f))) 100 6 60 0.4 0.5) 0)
(pin (measure "recur-local-self-send"
              (fn [j]
                (lcl-self-send 3)
                (get (chan/recv lcl-rcv) 1)) 100 6 60 0.4 0.5) 0)

# ── The scheduler frontier — a spawned fiber's round trip ─────────────
# `ev/spawn` + `ev/join` is the shape every structured-concurrency program
# is built out of, and it is the one the h2 corpus multiplies: one session
# answering 320 requests held ~1 GB of live heap on this round trip alone.
#
# What stranded, read off `--trace=rc` for one op: the fiber's own region,
# the closure it was made from, and the `[ok? value]` pair the join
# delivered — each left at rc=1, its birth reference never released. The
# frame that owned each one handed it to another fiber on ONE path and
# reached its end on every other: `wake-select-waiters` takes the completed
# fiber by tail-call move and resumes a select waiter with it, and a
# program with no select outstanding never takes that arm. The release the
# branch-arm window would anchor at the merge was refused because the
# region crosses the fiber frontier — a refusal the crossing's own count
# retires (docs/impl/region/mechanism.md § "A fiber crossing is a counted
# holder too"). A CLOSED control now, and the shape is gauged directly by
# tests/elle/region-fiber-frontier-window.lisp.
#
# The SCHEDULER's half of the per-fiber cost is closed and stays closed:
# a delivered join retires the completion records that used to hold every
# fiber a program ever spawned (docs/scheduler.md § Completion records,
# pinned by tests/elle/sched-completion-records.lisp).
(pin (measure "spawn-join" (fn [j] (ev/join (ev/spawn (fn [] 7)))) 100 6 60 0.4
              0.5) 0)

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
# `recur-local-*` probes read 0) and NOT, mostly, cons cells. `fold`/`reduce`
# `(->array coll)` once and INDEX-walk through the shared self-recursive
# `core-fold-step` driver (core.lisp), so neither the first/rest copy-scratch nor a
# per-call `go` closure exists to leak.
#
# `stdlib-concat` and `stdlib-fold` are CLOSED controls (undeclared, like
# `rest-array-copy`). Two readings of one window close them. The accumulator
# `concat` fills is `push-all`'s returned parameter, whose release the branch-arm
# window anchors where every arm reaches it. And `core-fold-step`'s own accumulator
# is a returned parameter the recursive arm hands its callee only through the
# COMBINER's result — a point the callee cannot reach it at, which is exactly where
# no funding edge is owed (docs/impl/region/mechanism.md § "The callee's return
# mint, and why the point owes it nothing"), so each displaced accumulator is freed
# per step instead of stranded. A regression to open must trip the completeness gate
# loudly.
(pin (measure-core "stdlib-concat" (stmt-run (fn [] (concat "a" "b")))
                   count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "stdlib-fold"
                   (stmt-run (fn [] (fold (fn [_ b] b) nil (list "x" "y"))))
                   count-gauge 100 6 60 0.4 0.5) 0)

# ── HOF composition — the zip-tower witness ───────────────────────────
# `zip-tower` is a zip built as a TOWER of higher-order calls: it converts every
# input to a list (`map to-list`), then recurses building the result with `(map
# first lists)` AND `(map rest lists)` at every step, then rebuilds an array. It is
# `map`/`pair`/`reverse`/`push` stacked several deep, so it measures whether COMPOSING
# higher-order calls compounds the collection-builder over-keep — a rate that scales
# with composition DEPTH rather than staying a per-call constant. It is a CLOSED
# control now (undeclared, like `rest-array-copy`), so a regression to open trips the
# completeness gate loudly rather than being absorbed under a root.
#
# The last mechanism it needed was a placement one. Each of the tower's helpers is a
# cell-free self-recursive `letrec` closure whose demise the binder carries out to the
# `Letrec` node, and every stdlib entry point the tower calls dispatches through a
# branch whose arms tail-call out — so that release was emitted at a merge label no arm
# arrives at. The frame-exit relocation replicates it ahead of each arm's `TailCall`,
# which needs the closure's VALUE route, the slot its `letrec` binder recorded
# (docs/impl/region/mechanism.md § "Self-cancelling is a property of the ROUTE, not of
# the region's class"). Shrink-only, and a SCALAR: the pin was a cross-tier [lo hi]
# range while the layers' arg-position closure-call results rode a ReturnValue retain
# the VM held and the JIT did not. Both tiers now measure the same rate, so the range
# collapses; re-open it as `[lo hi]` only if a tier span reappears.
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
                   count-gauge 100 6 60 0.4 0.5) 0)

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

# ── The injected abort delivery ───────────────────────────────────────
# `fiber/abort` installs a payload the CALLER owns, whose one reference answers
# the caller's ARGUMENT release and nothing else. So the injection mints the
# delivery — once, at the seam every route leaves through — and exactly one
# further release consumes it as a RESULT (docs/impl/region/effects.md
# § `Delivers`). Which release that is depends on where the injected error
# stops, and the routes are gauged apart because a mint keyed on the route
# rather than on the injection funds two of them twice.
#
# Nine CLOSED controls (undeclared, like `rest-array-copy`), one per route and
# per recorded mint:
#
#   `abort-masked`   — the fiber's mask catches, the caller releases the result;
#   `abort-escape`   — the error leaves the fiber, an ancestor `try` absorbs it;
#   `abort-caught`   — a handler INSIDE the body catches, and its own resume
#                      result is the consumer;
#   `abort-own-error`— that body then raises an error of its OWN, which mints its
#                      own delivery, so the abort owes the result nothing;
#   `abort-reraise`  — the body re-raises the injected payload, where value
#                      identity alone cannot tell the two apart;
#   `abort-defer`    — the unwind replays a parked `defer` frame, whose
#                      suspending call runs the result release;
#   `abort-held`     — the fiber is aborted with a value it was already handed,
#                      so its abandoned frame owes that value a release, which
#                      the recorded mint is what stops exempting;
#   `abort-other`    — its pair-control, aborted with a value the fiber does not
#                      hold, isolating the record from the walk;
#   `abort-aborting-frame` — the other side of the record: a literal
#                      materialized straight into the `fiber/abort` argument
#                      lives in the ABORTING frame's slot and nowhere else.
#
# Every one of them discards the abort's result. That is not incidental — see
# the tail-position pair below, which is what the discard is holding constant.
# The soundness complement is `region-fiber-abort-delivery-uaf.lisp`.
(defn ab-mk-caught []
  (fiber/new (fn []
               (protect (yield 1))
               7) |:yield :error|))
(defn ab-mk-masked []
  (fiber/new (fn []
               (yield 1)
               2) |:yield :error|))
(defn ab-hold-then-yield [q]
  (yield q)
  2)
(println "── folded suite: injected abort delivery ──")
(pin (measure-core "abort-masked"
                   (stmt-run (fn []
                               (let [f (ab-mk-masked)]
                                 (fiber/resume f)
                                 (fiber/abort f [1 2 3])
                                 nil))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "abort-escape"
                   (stmt-run (fn []
                               (let [p {:error :injected}
                                     f (fiber/new (fn []
                                       (yield 1)
                                       2) |:yield|)]
                                 (fiber/resume f)
                                 (try
                                   (begin
                                     (fiber/abort f p)
                                     nil)
                                   (catch e nil))
                                 nil))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "abort-caught"
                   (stmt-run (fn []
                               (let [f (ab-mk-caught)]
                                 (fiber/resume f)
                                 (fiber/abort f [1 2 3])
                                 nil))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "abort-own-error"
                   (stmt-run (fn []
                               (let [f (fiber/new (fn []
                                       (protect (yield 1))
                                       (error {:own 1})) |:yield :error|)]
                                 (fiber/resume f)
                                 (fiber/abort f [1 2 3])
                                 nil))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "abort-reraise"
                   (stmt-run (fn []
                               (let [f (fiber/new (fn []
                                       (let [r (protect (yield 1))]
                                         (error (get r 1)))) |:yield :error|)]
                                 (fiber/resume f)
                                 (fiber/abort f [1 2 3])
                                 nil))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "abort-defer"
                   (stmt-run (fn []
                               (let [f (fiber/new (fn []
                                       (defer
                                         (length [1 2 3 4 5])
                                         (yield 1)
                                         2)) |:yield :error|)]
                                 (fiber/resume f)
                                 (fiber/abort f [7 8 9])
                                 nil))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "abort-held"
                   (stmt-run (fn []
                               (let [p {:a 1}
                                     f (fiber/new ab-hold-then-yield
                                     |:yield :error|)]
                                 (fiber/resume f p)
                                 (fiber/abort f p)
                                 nil))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "abort-other"
                   (stmt-run (fn []
                               (let [p {:a 1}
                                     f (fiber/new ab-hold-then-yield
                                     |:yield :error|)]
                                 (fiber/resume f p)
                                 (fiber/abort f {:b 2})
                                 nil))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "abort-aborting-frame"
                   (stmt-run (fn []
                               (let [f (fiber/new (fn []
                                       (yield 1)
                                       2) |:yield|)]
                                 (fiber/resume f)
                                 (try
                                   (begin
                                     (fiber/abort f {:e 1})
                                     nil)
                                   (catch e nil))
                                 nil))) count-gauge 100 6 60 0.4 0.5) 0)
# The TAIL-position face of the same abort. The nine controls above all discard
# the abort's result; this one RETURNS it, which is the whole difference —
# `abort-tail-discarded` is the identical body with a `nil` after the call, so
# the gap between the pair isolates the tail position rather than the abort.
# `fiber/abort` in tail position is a NATIVE tail call that leaves by a signal,
# but an ABSORBED outcome is the carrier's answer rather than an exit: the frame
# is still there, so it falls through to the post-`TailCall` block and runs the
# owned-argument releases that block holds (docs/impl/region/mechanism.md § "A
# carrier that comes back with a result never left the frame"). A closed control
# now. The counter-factual — reading the answer as an exit — strands the fiber
# argument, the closure behind it, and the payload: three regions, of which an
# IMMEDIATE payload removes one and a payload an enclosing binding owns removes
# none. That is the discriminator against `abort-mask-caught-literal` below,
# whose whole strand IS the payload, and it is why the two must stay a pair.
#
# It reads 0 on every tier, and did so under `--jit=eager` before the fall-through
# landed: a compiled frame reaches the same releases through its own
# post-`TailCall` block, so the strand was interpreter machinery alone.
(pin (measure-core "abort-tail-result"
                   (stmt-run (fn []
                               (let [f (ab-mk-caught)]
                                 (fiber/resume f)
                                 (fiber/abort f [1 2 3])))) count-gauge 100 6 60
                   0.4 0.5) 0)
(pin (measure-core "abort-tail-discarded"
                   (stmt-run (fn []
                               (let [f (ab-mk-caught)]
                                 (fiber/resume f)
                                 (fiber/abort f [1 2 3])
                                 nil))) count-gauge 100 6 60 0.4 0.5) 0)
# The payload's OWN region, where one frame both allocates it and consumes the
# abort's result. The frame owes TWO releases on that one region — the argument
# and the result — and holds two references to fund them: its allocation's, and
# the injection's delivery mint. The post-`TailCall` block carries both, which is
# what a skipped block costs one region per abort. This probe needs all three
# ingredients — the fiber's MASK catches (so the caller receives the payload
# back), the payload is a literal materialized in the aborting frame, and that
# frame consumes the result — and `abort-mask-caught-bound` is the pair-control
# that removes the second, the same abort over a payload an enclosing binding
# owns, whose own release then covers it. A closed control now.
#
# It is NOT `abort-tail-result` seen smaller, and the two must stay a pair because
# resemblance is all there is to go on otherwise: both need the result in tail
# position and both flatten when it is bound. An IMMEDIATE payload separates them
# — it has no region at all, so this shape reads 0 there while
# `abort-tail-result` still counts the fiber. `abort-discard` above sits one mask
# bit away: with `|:yield|` the injected error escapes the fiber instead of being
# caught by the mask, and that route reclaims.
(defn ab-mk-mask-caught []
  (fiber/new (fn []
               (yield 1)
               9) |:yield :error|))
(pin (measure-core "abort-mask-caught-literal"
                   (stmt-run (fn []
                               (let [f (ab-mk-mask-caught)]
                                 (fiber/resume f)
                                 (protect (fiber/abort f "boom"))))) count-gauge
                   100 6 60 0.4 0.5) 0)
(pin (measure-core "abort-mask-caught-bound"
                   (stmt-run (fn []
                               (let [p "boom"
                                     f (ab-mk-mask-caught)]
                                 (fiber/resume f)
                                 (protect (fiber/abort f p))))) count-gauge 100
                   6 60 0.4 0.5) 0)
# `fiber/refuse` shares the injection seam with `fiber/abort`
# (`inject_error_at_suspension`) and leaves by the same `SIG_ABORT`, so it reaches
# the absorbed-carrier fall-through by the same route and needs its own reading:
# nothing about the seam distinguishes the two, so a change that reintroduces the
# strand for one reintroduces it for both. The pair is the same as the abort's —
# the result RETURNED, and the identical body with a `nil` after the call.
(pin (measure-core "refuse-tail-result"
                   (stmt-run (fn []
                               (let [f (ab-mk-caught)]
                                 (fiber/resume f)
                                 (fiber/refuse f [1 2 3])))) count-gauge 100 6
                   60 0.4 0.5) 0)
(pin (measure-core "refuse-tail-discarded"
                   (stmt-run (fn []
                               (let [f (ab-mk-caught)]
                                 (fiber/resume f)
                                 (fiber/refuse f [1 2 3])
                                 nil))) count-gauge 100 6 60 0.4 0.5) 0)

# ── The physical-id dimension ─────────────────────────────────────────
# What a CALL costs in physical region ids, the dimension every probe above is
# blind to. A native call mints a physical region for its result before the
# callee runs, because the callee may allocate the result into it; a callee that
# returns an immediate, or a value borrowed from an argument, allocates nothing
# into that id. It never becomes a live region, so no teardown can return it, and
# it holds no object, no page, no bytes and no reference count for any other
# gauge to see (docs/impl/region/model.md § "Physical id recycling"). What it
# costs is resident: the region table is a `Vec` indexed by physical id, so the
# largest id ever made live sets its length.
#
# Both exits of the id lifecycle are gauged here, and the pair must stay
# together — the recycle admits an id only where the mint never materialized it,
# so an id that DID materialize has to reach the free list by its teardown
# instead, and a probe of one exit alone cannot tell a working recycle from one
# that hands the same id back twice. `id-immediate-result` and `id-const-compare`
# are results a native never allocates at all; `id-borrowed-element` and
# `id-borrowed-index` are results borrowed out of an argument; `id-fresh-result`
# is the materializing control, whose id comes back by the ordinary teardown.
#
# These are CLOSED controls (undeclared, like `rest-array-copy`), so a regression
# to open trips the completeness gate loudly. Read them against the id
# discriminator above: an id gauge that cannot move reads 0 for all five.
(def id-hold [1 2 3])
(println "── folded suite: physical-id recycling ──")
(pin (measure-core "id-const-compare" (stmt-run (fn [] (< 1 2))) ids-gauge 100 6
                   60 0.4 0.5) 0)
(pin (measure-core "id-immediate-result" (stmt-run (fn [] (length id-hold)))
                   ids-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "id-borrowed-index" (stmt-run (fn [] (get id-hold 0)))
                   ids-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "id-borrowed-element" (stmt-run (fn [] (first (pair 1 2))))
                   ids-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "id-fresh-result" (stmt-run (fn [] (pair 1 2))) ids-gauge 100
                   6 60 0.4 0.5) 0)

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
(pin-yield "yield-concat" (fn [i] (concat "x" (number->string i))) 0)
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
# (per-region RC) path — the always-Shared class. The send seam increfs the
# message region at the enqueue to hold it in the buffer until received ("a store
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
# MODULE-level sink, `probe-disc`, and is unaffected). `push-accum` is the same
# accumulator fed the per-op `map` scratch (§ F1a), and reclaims for the same
# reason once that scratch does: `map` dispatches through a `cond` whose later
# clause TESTS held the collection's one release, and a clause test is a
# conditional position exactly as a clause body is
# (docs/impl/region/mechanism.md § "An arm is a conditional position, not a
# syntactic arm body"). Its kernel CAPTURES `k` deliberately — a capture declines
# loop fusion, so the real stdlib `map` runs and there is a per-op scratch to
# measure; a fusable kernel has none, which is what the dissolution controls
# (`map-while`) measure. A CLOSED control now, undeclared like `rest-array-copy`.
# `struct-outer` is the fn-local reassign-1-slot control: a loop-carried cell whose
# content is re-minted every iteration, bounded by the overwrite + demise pair (F5).
# `string-outer`/`append-outer` are CLOSED controls for the same close
# `stdlib-concat` gauges: each iteration's `concat`/`append` returns `push-all`'s
# accumulator parameter, and the branch-arm window anchors its release where every
# arm reaches it. Their rate was always flat per-iter, never accumulator growth, so
# a regression to open is a per-call strand and must trip the completeness gate.
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
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
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
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "append-outer"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @acc [])
                     (def @j 0)
                     (while (%lt j b)
                       (assign acc (append acc [j]))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)

# ── The loop-carried accumulator a function RETURNS ───────────────────
# `loop-acc-return` builds a list by reassigning a local across a `while` and
# hands it back; `recur-acc-return` computes the same list by threading the
# accumulator as a parameter to a self-recursive binding. Same value, same
# allocations, one difference: which construct carries the accumulator, so the
# pair prices the rewrite a programmer would otherwise have to make.
#
# The returned binding takes the 1-slot-container model, so each value the loop
# displaces dies at the overwrite and the final content leaves with the caller —
# the `Return`'s mint pays for the caller's reference and the cell's content drop,
# emitted after that mint, releases the cell's. Withhold the container half and
# each stored value is protected only by its producer's one release, which the
# returned-region extension drags out to the `Return`: it names whatever the
# producer's ANF slot holds LAST and every earlier value is stranded, one region
# per trip. Neither probe cares whether the accumulated values are
# self-referential (a cons chain and a fresh string per iteration read alike), and
# neither is about closure capture — a helper closing over the accumulator, or
# over a mutable input container, is the `capture-acc-*` pair below.
(defn loop-acc-return-shape [n]
  (def @acc ())
  (def @i 0)
  (while (%lt i n)
    (assign acc (pair i acc))
    (assign i (%add i 1)))
  acc)
(def recur-acc-return-step
  (fn [i n acc]
    (if (%lt i n) (recur-acc-return-step (%add i 1) n (pair i acc)) acc)))
(defn recur-acc-return-shape [n]
  (recur-acc-return-step 0 n ()))
(pin (measure "loop-acc-return" (fn [j] (length (loop-acc-return-shape 4))) 100
              6 60 0.4 0.5) 0)
(pin (measure "recur-acc-return" (fn [j] (length (recur-acc-return-shape 4)))
              100 6 60 0.4 0.5) 0)

# ── The captured mutable accumulator — the shape a builder is WRITTEN in ──
# Every container probe above drives its accumulator from a bare `while` in the
# same scope. The everyday builder does not: it names the walk, and that helper
# CAPTURES the mutable accumulator it fills. Both realizations are here, because
# they take different region paths — `capture-acc-letrec` closes a local
# `letrec` helper over `out` (a capture cell plus a closure per call),
# `capture-acc-while` closes a plain `let`-bound `fn` over it — and both must be
# bounded on the SAME terms as the uncaptured form, or a builder is only
# leak-free when its author threads the accumulator through parameters instead.
# `thread-acc-param` is that parameter-threaded alternative, kept beside them as
# the discriminator: were the captured forms to regress, this one would stay at 0
# and the difference would be exactly the cost of the rewrite. Closed controls,
# undeclared like `rest-array-copy`, so a regression trips the completeness gate
# rather than being absorbed under a root. The payload is a heap string per push,
# so a stranded accumulator shows as element growth and not merely one region.
(def thread-acc-driver
  (fn [out i n]
    (when (%lt i n)
      (push out (string "e" i))
      (thread-acc-driver out (%add i 1) n))))
(pin (measure-core "capture-acc-letrec"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (let [out @[]]
                         (letrec [go (fn [i]
                                       (when (%lt i 4)
                                         (push out (string "e" i))
                                         (go (%add i 1))))]
                           (go 0))
                         (length out))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "capture-acc-while"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (let [out @[]
                             fill (fn [n]
                                    (def @i 0)
                                    (while (%lt i n)
                                      (push out (string "e" i))
                                      (assign i (%add i 1))))]
                         (fill 4)
                         (length out))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "thread-acc-param"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (let [out @[]]
                         (thread-acc-driver out 0 4)
                         (length out))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)

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
# rather than being absorbed as an F5 strand. Their counterfactual and the two
# window boundaries are `tests/elle/region-branch-arm-window.lisp`; the soundness
# complement is `region-branch-arm-window-uaf.lisp`.
# `branch-arm-tailcall-sibling` is the third: the same window over a branch whose
# OTHER arm leaves through a frame-replacing closure tail call. Declining such a
# branch whole strands the argument on the arm driven here, which is what the
# `concat`/`append` family paid per call.
# `arm-alias-inside` is the fourth: the same window over a branch one of whose
# arms binds an ALIAS of the argument. The live-in premise is about the
# allocation, so a binding the arm introduces is not a birth in the arm; reading
# the two kinds of anchor as one strands the argument on every other arm.
# `arm-seq-read` is the fifth: the same window over a branch one of whose arms
# READS the argument through a sequence read. Those reads declare `Opaque`, so
# they seed nothing on escape's store facet and the window admits the argument;
# a `Mixed` declaration seeds one and holds the whole branch to the baseline.
# `arm-loop-read` is the sixth: the same window over a branch one of whose arms
# LOOPS over the argument. Its release is anchored at the loop node, which the
# lowerer emits after the loop, so the boundary that declines a loop is the loop's
# BODY and this class is admitted; reading the boundary as the closed subtree
# interval strands the argument on every arm but the looping one.
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
(pin (measure-core "arm-alias-inside"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t22-arm-alias-inside (list 1 2 3) :a)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "arm-seq-read"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t22-arm-seq-read (list 1 2 3) :a)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "arm-loop-read"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t22-arm-loop-read (list 1 2 3) :a)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "branch-arm-tailcall-sibling"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t22-tailcall-sibling (list 1 2 3) :a)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "branch-arm-return-captured"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t22-returned-captured (@string) "xy")
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
# The scope-map face of the same `Match`: the arm READS a name its pattern bound,
# and that read is a borrowing read of the SCRUTINEE (rules.md Rule 4), so it is
# what places a whole fresh struct's release. A pattern whose scope goes
# unrecorded reads as bound outside this loop, hoisting that release past the loop
# and stranding every iteration's scrutinee but the last
# (docs/impl/region/mechanism.md § "Every binder records its scope"). CLOSED
# control — undeclared, like `param-used-arm`, so a regression trips the
# completeness gate loudly rather than being absorbed as an F5 strand. The
# per-shape rows and the arm-not-taken / guard / nested-loop faces are
# `tests/elle/region-match-bind-loop.lisp`; the soundness complement is
# `region-match-bind-loop-uaf.lisp`.
(pin (measure-core "struct-match"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (match {:type :a :v j}
                         {:type :a :v v} v
                         _ 0)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
# The frame-exit release, eleven CLOSED controls. A frame-replacing tail call means
# everything the lowerer emits after it runs only on the NATIVE fall-through, so a
# release landing there is emitted where control may never arrive; the close moves
# that one release ahead of the `TailCall` — admitted where escape proves the frame
# holds the region alone, since on the closure path it fires where none fired
# before (docs/impl/region/mechanism.md § "A release past a frame-replacing tail
# call is not a release"). `tail-frame-exit-unused` is the unused-parameter
# fallback through that dead block; `tail-frame-exit-arms` is the same strand one
# block further out, where the tail calls sit in the arms of a branch and the
# release lands past the merge; `tail-frame-exit-captured` is the holder the tail
# callee reaches through its CAPTURED environment, admitted because the funnel
# counted that hold; `tail-frame-exit-handback` is that same edge carrying one step
# further — the callee RETURNS the parameter, so the caller's reference is minted
# inside it, after the relocated release has run, and the env edge is what holds
# the region off zero in between; `tail-frame-exit-moved` is the exemption face,
# already 0, which reads GROWTH if the hoist ever releases an argument the callee
# now owns. The two `tail-frame-exit-fwd-cell*` probes are the region no holder
# binding NAMES — a prebound forward cell, which carries its binding's verdict one
# indirection out, for a frame-local capturer and for a returned one in turn;
# `tail-frame-exit-fwd-cell-sib` is the inversion where the sibling captures the
# member, so one tail call strands a merged arena AND its callee, on two channels
# that neither substitute for one another nor name the same region.
# `tail-frame-exit-operand-value` is the reading of what the call itself names: a
# region the tail call reaches through no operand's VALUE, only through an argument's
# own nested call, is not exempt. `tail-frame-exit-callee-member` is the other side
# of the exemption: what it keeps in the dead block, a channel must still run, so a
# tail callee whose release the enclosing letrec places at its SCOPE END rides the
# deferral exactly as one demising at the call node does.
# `tail-frame-exit-fold-driver` is the other end of the hand-back's enumeration: a
# returned accumulator the tail callee reaches through NEITHER route, so its
# `Return` mints nothing against the region and the relocated release is the last.
# Undeclared, like `param-used-arm`, so a regression trips the
# completeness gate loudly rather than being absorbed as F1a scratch. The
# counterfactual and the boundary rows live in
# `tests/elle/region-tail-frame-exit.lisp`; the soundness complement is
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
(pin (measure-core "tail-frame-exit-captured"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t23-captured (list 1 2 3))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "tail-frame-exit-handback"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t23-drive-handback (list 1 2 3))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "tail-frame-exit-moved"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t23-moved (list 1 2 3))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "tail-frame-exit-fwd-cell"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t23-fwd-cell 3)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "tail-frame-exit-fwd-cell-ret"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t23-fwd-cell-ret 3)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "tail-frame-exit-fwd-cell-sib"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t23-fwd-cell-sib 3)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "tail-frame-exit-operand-value"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t23-operand-value 3)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "tail-frame-exit-callee-member"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t23-callee-member 3)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "tail-frame-exit-fold-driver"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (t23-fold-drive 3)
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

# ── Thunk-run native results ──────────────────────────────────────────
# A native can produce its result by running compiled code on the driving VM:
# `import` runs the module body, and `arena/allocs` runs the measured thunk.
# Such a result already carries its return mint. The dispatch pass-through
# retain must not fund the caller a second time (`result_minted`,
# docs/impl/region/effects.md § "Native region effects"). `arena/allocs`
# embeds its thunk's result in a fresh pair, so the boundary consumes the
# mint after the pair's alloc-scan counts the embedding. Both probes are
# CLOSED controls (undeclared, like `rest-array-copy`). Before the
# accounting fix each read ~3/op — the returned closure, its letrec arena,
# and a capture cell, stranded per call — so a regression to open trips the
# completeness gate loudly. The discarded-statement shape needs the DIRECT
# while run-block (a thunk's return convention would mask the over-keep).
(def import-module-dir (file/mktempdir))
(def import-module-path (string import-module-dir "/oracle-import-mod.lisp"))
(spit import-module-path
      (string "(elle/epoch 12)\n" "(defn f1 [x] (+ x 1))\n"
              "(fn [] {\"f1\" f1})\n"))
(defn import-thunk []
  (defn g1 [x]
    (+ x 1))
  (fn [] {"g1" g1}))
(println "── folded suite: thunk-run native results ──")
(pin (measure-core "import-result"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (begin
                         (import import-module-path)
                         nil)
                       (assign j (%add j 1)))) count-gauge 25 6 40 0.4 0.5) 0)
(pin (measure-core "allocs-result"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (begin
                         (arena/allocs import-thunk)
                         nil)
                       (assign j (%add j 1)))) count-gauge 50 6 40 0.4 0.5) 0)
(delete-file import-module-path)
(delete-directory import-module-dir)

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
# The closure-as-module's accessor still reaches its captured value after the
# module's frame is gone. `module-cell-read-window` above prices the fallback the
# frame-exit relocation takes for this shape; this is the property that fallback
# exists to keep, and it is a value assertion because neither memory gauge can
# see it — the box release and the release routed through the box are correctly
# COUNTED either way, so an inverted pair reads flat here and clean under
# `--trace=guardfree`. The emission order itself is stated over the finished
# blocks by `lir::lower::assert_cells_outlive_their_readers`, which runs in every
# debug build over every block it lowers.
(check (assert (= ((get (mod-cell-immediate) :p)) (ptr/from-int 7))
               "closure-as-module accessor read back its Immediate-init capture"))
(check (assert (= ((get (mod-cell-heap) :p)) "cap")
               "closure-as-module accessor read back its heap-init capture"))

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
                       " ≠ 3 — the growth probes (the object-count and "
                       "physical-id live-growth discriminators, the sub-integer "
                       "estimator self-test) must each read open")))

(report)
(println "oracle: ok")
