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

(defn show [r]
  "Print one measured class as a dashboard line."
  (let [alt (get r :alt-rate)]
    (println "  " (get r :verdict) "  " (get r :label) ": rate=" (get r :rate)
             " ±" (get r :half)
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
# cell↔closure cycle — reclaimed per call by ordinary RC / the tail-call adopt,
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
(def io (measure-stable "io-yield ev/sleep" probe-io-yield 200 8 80 0.4 0.5))
(show io)
(check (assert (not= (get io :verdict) :contaminated)
               (string "io-yield rate is block-dependent (B vs 2B): "
                       (get io :rate) " vs " (get io :alt-rate)
                       " — a per-block artifact, not a per-op rate")))
(check (assert (= (get io :verdict) :open)
               (string "io-yield not measured as leaking: " (get io :verdict))))
(check (assert (and (< 1.5 (get io :rate)) (< (get io :rate) 2.5))
               (string "io-yield rate " (get io :rate)
                       " ∉ [1.5,2.5] — shrink-only")))

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
# One dashboard covering every leak class (memory.md § 4), on the estimator. The
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
        a)) 1] ["mut-struct" (fn [j] @{:x j}) 0]
   ["mut-string"
    (fn [j]
      (let [s @""]
        (push s "x")
        s)) 2]
   ["nested-loop"
    (fn [j]
      (def @k 0)
      (while (%lt k 10)
        {:x j :y k}
        (assign k (%add k 1)))) 0]  # collection ops
    ["reduce" (fn [j] (reduce + 0 [1 2 3])) 3]
   ["fold" (fn [j] (fold (fn [a x] (+ a x)) 0 [1 2 3])) 5]
   ["zip" (fn [j] (zip [1 2] [3 4])) 10] ["sort" (fn [j] (sort [3 1 2])) 0]
   ["reverse" (fn [j] (reverse [1 2 3])) 2]  # F1a witness (memory.md § F1). `(rest array)` copies the tail into a fresh
   # immutable array slice whose call-result region is never reclaimed — the
   # transform-scratch root the whole HOF family rides: `fold`/`reduce` eagerly
   # `(->array coll)` then walk with `first`/`rest`, so even a list leaks one slice
   # per element. `(rest list)` shares its tail and reads 0. Shrink-only.
   ["rest-array-copy" (fn [j] (rest [1 2 3 4 5])) 1]
   ["distinct" (fn [j] (distinct [1 2 1 3])) 3]
   ["take-drop"
    (fn [j]
      (take 2 (list 1 2 3))
      (drop 1 (list 1 2 3))) [5 6]]
   ["group-by" (fn [j] (group-by odd? [1 2 3 4])) 4]
   ["frequencies" (fn [j] (frequencies [1 2 1 3])) 3]
   ["to-array" (fn [j] (->array (list 1 2 3))) 0]
   ["to-list" (fn [j] (->list [1 2 3])) 0]
   ["freeze" (fn [j] (freeze @[1 2 3])) 0]
   ["slice" (fn [j] (slice [1 2 3 4] 1 3)) 0]  # trailing nil keeps the body's value a discarded STATEMENT, matching the
   # original while-loop (where the alloc is never the loop's tail value)
   ["keys-values"
    (fn [j]
      (keys {:a 1 :b 2})
      (values {:a 1 :b 2})
      nil) 0] ["merge" (fn [j] (merge {:a 1} {:b 2})) 5]
   ["struct-lit" (fn [j] {:x j :y (+ j 1)}) 0]
   ["struct-get"
    (fn [j]
      (let [s {:x j}]
        s:x)) 0]
   ["struct-put"
    (fn [j]
      (let [s @{:x 0}]
        (put s :x j))) 1]
   ["push-churn"
    (fn [j]
      (let [items @[]]
        (push items {:k j}))) 2]  # The capture-back-edge cycle: a container captured by a closure it holds
   # (`m ⊇ c` store, `c ⊇ m` capture). Per-region RC cannot collect the m↔c
   # cycle, and no region root can own it (the captured member's live decref
   # over-extends past the closure), so it leaks per op. The activation-owner cut
   # reclaims the INTRINSIC form of this shape (runtime::tests::ownership::
   # region_ownership_capture_back_edge_cycle_reclaims, without_stdlib /
   # %array-push), but here the full-stdlib `push`/`length` wrappers keep the
   # containment out of the cut's reach, so it stays on the RC baseline — a
   # promptness residual, shrink-only.
   ["capture-backedge"
    (fn [j]
      (let [root @[]
            m @[]]
        (let [c (fn [] (length m))]
          (push m c)
          (c)
          (push root m)
          nil))) 4]  # The transferred returned cycle: a helper builds an a<->b cycle and hands
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
   ["concat" (fn [j] (concat "a" "b" "c")) 13]
   ["split" (fn [j] (string/split "a,b,c" ",")) 0]
   ["join" (fn [j] (string/join ["a" "b" "c"] ",")) 0]
   ["trim" (fn [j] (string/trim "  x  ")) 0]
   ["replace" (fn [j] (string/replace "hello" "l" "r")) 0]
   ["num-to-str" (fn [j] (number->string j)) 0] ["read" (fn [j] (read "42")) 0]
   ["call-chain" (fn [j] (helper-f (helper-g (helper-h j)))) 0]  # A fresh closure-call result passed as an ARGUMENT into a stdlib closure
   # call (`pair`): the arg's ReturnValue retain. Under the unified-intrinsics
   # stdlib the callee's store funnel no longer consumes it — 1/op (it read 0,
   # balanced, before the unification; take-drop moved 5→6 by the same class).
   # The minimal member of the class the zip/take-drop/merge/struct-put/
   # yield-multimut/put-churn pins carry at larger multiplicities. Shrink-only.
   ["arg-result" (fn [j] (pair j (helper-g j))) 1]
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
    13]
   ["each-list"
    (fn [j]
      (each x in (list 1 2 3)
        {:val x})) 3]
   ["map-while"
    (fn [j]
      (map (fn [x]
             (numeric!)
             (%add x 1)) [1 2 3])) 5]
   ["filter-while"
    (fn [j]
      (filter (fn [x]
                (numeric!)
                (%gt x 1)) [1 2 3])) 5]
   ["nested-closure"
    (fn [j]
      (let [f (fn [] (fn [] j))]
        ((f)))) 2]  # user-fn calls, value flow
    ["user-struct" (fn [j] (make-struct j)) 0]
   ["user-string" (fn [j] (make-label j)) 0] ["chain" (fn [j] (process j)) 0]
   ["wrap-map"
    (fn [j]
      (map (fn [x]
             (numeric!)
             (%add x 1)) [1 2 3])) 5] ["factory" (fn [j] (t13proc j)) 0]
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
   ["store-wrapper" (fn [j] (t19-store t19s (string "v" j))) 1]
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
   ["concat-while" (fn [j] (concat "x" (number->string j))) 10]
   ["protect-while"
    (fn [j]
      (let [[ok v] (protect ((fn [] j)))]
        v)) 2]
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
        (fiber/resume f))) 3]
   ["multi-resume"
    (fn [j]
      (let [f (fiber/new (fn []
                           (yield 1)
                           (yield 2)
                           3) |:yield|)]
        (fiber/resume f)
        (fiber/resume f)
        (fiber/resume f))) 3]
   ["protect-call"
    (fn [j]
      (let [[ok v] (protect (+ 1 2))]
        v)) 0]
   ["yield-discard"
    (fn [j]
      (let [f (fiber/new (fn []
                           (yield {:x j})
                           99) |:yield|)]
        (fiber/resume f))) 4]
   ["never-resumed"
    (fn [j]
      (let [f (fiber/new (fn [] {:x j}) |:yield|)]
        f)) 0]
   ["denied-discard"
    (fn [j]
      (let [f (fiber/new (fn [] (println "blocked")) |:error :io| :deny |:io|)]
        (fiber/resume f)
        (get (fiber/value f) :error))) 10]  # A parked fiber hard-killed by `fiber/cancel`: the suspending resume's
   # carrier pass-through retain is released only by a COMPLETING resume
   # (`release_completed_resume_carrier`), and a cancelled fiber never
   # completes, so its fiber region (dragging closure + template) stays
   # retained — the yield-discard class at the cancel exit. The kill itself
   # frees everything the fiber OWNS (owner nodes — the terminal-fiber
   # teardown); this residue is the unreleased carrier retain, not owned
   # state. Shrink-only.
   ["cancel-discard"
    (fn [j]
      (let [f (fiber/new (fn []
                           (yield j)
                           9) |:yield|)]
        (fiber/resume f)
        (fiber/cancel f :dead)
        (fiber/status f))) 3]  # `fiber/abort` of a PARKED fiber (memory.md § F2). Abort injects an error
   # and resumes the fiber for unwinding; with no in-body handler it lands `:error`,
   # which the model keeps RESUMABLE (the restarts system) — so the terminal teardown
   # (`take_fiber_owned`/`release_fiber_owned`) never fires and the fiber's parked
   # activation node + region-map borrows + operand stack strand, plus the fresh error
   # struct the abort mints. `protect` catches the propagated error. A hard `fiber/cancel`
   # of the same shape (`cancel-discard`, 3) routes through `kill_fiber` and frees the
   # owned set — the gap is precisely the resumable-`:error` non-teardown. Shrink-only:
   # closes when a discarded `:error` fiber is routed through the terminal teardown.
   ["abort-discard"
    (fn [j]
      (let [f (fiber/new (fn []
                           (yield j)
                           9) |:yield|)]
        (fiber/resume f)
        (protect (fiber/abort f "boom")))) 8]])

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
# tail-call adopt (lir/lower/control/call.rs `tail_callee_adopts`). The HOF pins above
# (map/reduce/zip/…) ride this same cell-free mechanism — their `go` helpers.
#
# MUTUAL recursion (`recur-local-mutual`) is reclaimed (rate 0): `ev`/`od` each capture
# the OTHER, a genuine closure↔closure cell cycle — but an immutable lambda-initialized
# letrec binding's forward cell is a compiled static-slot cell in every position, so the
# closure-cycle merge collapses the SCC + cells onto one arena in-lambda exactly as at
# top level. The tail-call letrec body `(ev n)` strands the binding-scope drop; the
# tail-call adopt releases the merged arena once at the recursion's normal completion
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
# release rides the explicit arena adopt (`TailCall::adopt_region_slot`,
# `RegionInfo::cycle_tail_adopt`): a closure callee (`+`) adopts the arena at
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

# ── The per-call object MINT (a second axis: mint count, not leak rate) ──
# `recur-local-self` above pins the LEAK rate (0 — the cell-free closure is reclaimed
# per call). These two pin the orthogonal axis: how many heap objects a self-recursive
# local closure MINTS per enclosing call. A leak-rate probe is blind to per-call minting
# that reclaims; it is made visible by RETAINING each closure in a block-local @keep and
# reading object growth.
#
# A retained self-recursive closure pins TWO objects/call — the closure and its one-entry
# env — with NO forward cell: the self-edge does not mark the binding captured, so a
# self-recursive `loop` is cell-free, its self-reference resolving to the executing
# closure (docs/impl/selfrec.md). The equal-arity foreign-capture CONTROL (captures the
# immediate n, not itself) also pins TWO — likewise cell-free. Their gap is therefore ~0:
# no per-call forward cell distinguishes them. Both shrink-only; a gap near 200 would mean
# a per-call cell was reintroduced for pure self-recursion. Object growth, not region
# growth, is the gauge (closure + env share one region, so region count is identical). The
# deterministic flip-gate is
# runtime::tests::ownership::self_recursive_loop_is_cell_free.
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
                   60 0.4 0.5) 2)

# ── Stdlib / native-tail / discarded-tail leak classes ────────────────
# Three more leak classes pinned in the one dashboard (memory.md §5 — leak state
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

# Stdlib per-call leak (memory.md § F1a — the transform-scratch retain). The leaked
# objects are INTERMEDIATE scratch, NOT the recursive helper (which reclaims — the
# `recur-local-*` probes read 0) and NOT, mostly, cons cells: `fold`/`reduce` eagerly
# `(->array coll)` then walk with `first`/`rest`, minting a fresh array slice per
# element (`rest-array-copy` above), and `concat` builds a fresh accumulator + per-arg
# combiner closures. All are non-escaping, acyclic call-result regions no static slot
# can name. Pinned at the exact `(concat "a" "b")` / 2-element `fold` shapes.
(pin (measure-core "stdlib-concat" (stmt-run (fn [] (concat "a" "b")))
                   count-gauge 100 6 60 0.4 0.5) 10)
(pin (measure-core "stdlib-fold"
                   (stmt-run (fn [] (fold (fn [_ b] b) nil (list "x" "y"))))
                   count-gauge 100 6 60 0.4 0.5) 5)

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
# is the standing record of what that rewrite worked around.) Shrink-only. 25 → 32 under the
# unified-intrinsics stdlib: the layers' arg-position closure-call results ride the
# now-unconsumed ReturnValue retain (the `arg-result` class) at composition depth.
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
                   count-gauge 100 6 60 0.4 0.5) 32)

# Dispatch-wrapper passthrough leak (memory.md § F1b) — NOT a "native-tail double
# mint". The direct intrinsic `%put-struct` and the single non-dispatching native
# `del` both reclaim a fresh copy at 0/op (the CONTROLS: `native-tail-del-ctl` below,
# `put-slot-source` above), so the fresh copy is a red herring. The leak is the stdlib
# `put`/`push` `(match (type-of coll) …)` WRAPPER: its container arg + fresh result get
# ONE `decref_point` in the textually-last arm, so multi-arm usage strands them on the
# other paths — the open residual of the settled branch-compensation class
# (`branch_arm_decrefs` releases the stored value, not the container/result). `put` on
# an immutable aggregate also mints a fresh container (2/op vs 1/op when reused).
(pin (measure-core "native-tail-put-struct" (stmt-run (fn [] (put {:a 1} :b 2)))
                   region-gauge 100 6 60 0.4 0.5) 2)
(pin (measure-core "native-tail-put-array" (stmt-run (fn [] (put [10 20] 0 99)))
                   region-gauge 100 6 60 0.4 0.5) 2)
(pin (measure-core "native-tail-del-ctl" (stmt-run (fn [] (del {:a 1 :b 2} :a)))
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
#   `%pop` — the remove funnel balances (rate 0): a box store+rebind, an @set add, and
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
#   F1b remove-wrapper (memory.md § F1b) — the stdlib `pop`/`del` `(match (type-of coll)
#   …)` dispatch wrapper strands the container arg + fresh result on the arms the
#   textually-last arm does not reach, exactly as the STORE wrappers (put/push/set) do.
#   `pop` leaks (3): the leak is the multi-arm wrapper. Closes by the SAME mechanism as
#   the store half — per-arm compensation of the container+result, or dispatch prune on a
#   statically-typed scrutinee.
#
#   The raw remove-funnel residual — `%del` leaks in-place, even on an IMMEDIATE value
#   (raw-del-immediate reads 1, raw-del reads 2 = that 1 plus the removed heap member's
#   region): the @struct/@set remove native does not reach the balance `%pop`
#   demonstrates. Closes by bringing `%del`'s result/removed-value accounting to
#   `%pop`'s parity. Distinct from the F1b wrapper leak, which rides every remove op.
(println "── folded suite: mutable-store funnel (remove/rebind half) ──")
(pin (measure-core "box-rebind"
                   (stmt-run (fn []
                               (let [b (box (list 1 2))]
                                 (rebox b (list 3 4))))) count-gauge 100 6 60
                   0.4 0.5) 0)
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
(pin (measure-core "pop-wrapper"
                   (stmt-run (fn []
                               (let [a @[]]
                                 (push a (list 1 2))
                                 (pop a)))) count-gauge 100 6 60 0.4 0.5) 3)
(pin (measure-core "del-wrapper"
                   (stmt-run (fn []
                               (let [m @{}]
                                 (put m :k (list 1 2))
                                 (del m :k)))) count-gauge 100 6 60 0.4 0.5) 1)
(pin (measure-core "set-del-wrapper"
                   (stmt-run (fn []
                               (let [s @||]
                                 (add s 7)
                                 (del s 7)))) count-gauge 100 6 60 0.4 0.5) 1)
(pin (measure-core "raw-del"
                   (stmt-run (fn []
                               (let [m @{}]
                                 (%put m :k (%pair 1 2))
                                 (%del m :k)))) count-gauge 100 6 60 0.4 0.5) 2)
(pin (measure-core "raw-del-immediate"
                   (stmt-run (fn []
                               (let [m @{}]
                                 (%put m :k 7)
                                 (%del m :k)))) count-gauge 100 6 60 0.4 0.5) 1)

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
(pin-yield "yield-concat" (fn [i] (concat "x" (number->string i))) 10)
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
                   count-gauge 100 6 60 0.4 0.5) 1)
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
                   count-gauge 100 6 60 0.4 0.5) 2)
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
# across the block's ops. `push-outer`/`push-accum` are GENUINE growth — the
# accumulator retains every prior (a live-growth discriminator, not a defect; do not
# "fix" it). `struct-outer` is the fn-local reassign-1-slot over-keep (memory.md § F5).
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
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 1)
(pin (measure-core "push-accum"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @acc @[])
                     (def @j 0)
                     (while (%lt j b)
                       (push acc
                             (map (fn [x]
                                    (numeric!)
                                    (%add x 1)) [1 2 3]))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 5)
(pin (measure-core "struct-outer"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @last nil)
                     (def @j 0)
                     (while (%lt j b)
                       (assign last {:x j})
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 1)
(pin (measure-core "string-outer"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @s "")
                     (def @j 0)
                     (while (%lt j b)
                       (assign s (concat s "x"))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 9)
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
# convention would reclaim the break-escape the originals leak. break carries a
# value past the block's decref points (the accepted break-skipped over-keep).
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
# F1b (memory.md § F1b — the dispatch-wrapper passthrough leak). The raw intrinsic
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
# put-churn mints a FRESH container per op and hands it through the stdlib
# `put`; the container's region survives the discard and cascades its stored
# struct — 2/op in BOTH intrinsics modes, every tier (pure interpreter
# included). The slot-source probes above show the raw-%put container
# reclaiming, so the over-keep rides the stdlib-put dispatch route, not the
# store funnel itself. Shrink-only.
(pin (measure-core "put-churn"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (let [s @{}]
                         (put s :k {:v j}))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 2)
(pin (measure-core "struct-match"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (match {:type :a :v j}
                         {:type :a :v v} v
                         _ 0)
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 1)
(pin (measure-core "break-value"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (block (let [x (t17-h)]
                                (break x)))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 1)
(pin (measure-core "break-value-used"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (let [r (block (let [x (t17-h)]
                                        (break x)))]
                         (get r :a))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 1)
(pin (measure-core "break-value-lit"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (block (let [x {:a j}]
                                (break x)))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 1)

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

(report)
(println "oracle: ok")
