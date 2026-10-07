(elle/epoch 14)
# audited: 2026-10-05
## lib/ratchet/estimator.lisp — the adaptive per-op rate: an anytime-valid
## empirical-Bernstein estimator over blocks, read on any number of gauges in
## one drive.
## docs/ratchet.md
##
## Loaded by lib/ratchet.lisp: (def est ((import "std/ratchet/estimator")))
##
## A gauge is {:axis :unit :read :epsilon …}; `read` is an Immediate
## primitive, so a reading allocates nothing and cannot move what it reads.
## Nothing else may allocate inside a block's [before, after] window either:
## the arrays the readings land in are made once, ahead of every window, and
## the loops inside it advance by intrinsics.

(fn [& opts]
  # ── Empirical-Bernstein half-width ────────────────────────────────
  # Total error budget δ, spent across an unbounded number of peeks via a
  # per-m union bound: δ_m = δ·(6/π²)/m², so Σ_m δ_m ≤ δ and the interval is
  # valid at EVERY block boundary — no optional-stopping inflation from
  # "peek at the interval and stop when it looks tight".
  (def EB-DELTA 0.000001)
  (def INV-PI2-6 0.6079271018540267)

  (defn eb-halfwidth [m var rng]
    "Anytime-valid empirical-Bernstein half-width on the per-op-rate mean after
     m block samples with sample variance VAR and observed range RNG. The
     linear term uses the OBSERVED range, so a deterministic rate (rng 0,
     var 0) has half-width 0 and converges at the floor. Maurer–Pontil form."
    (if (< m 2)
      (math/inf)
      (let [dm (/ (* EB-DELTA INV-PI2-6) (* m m))
            l (math/log (/ 3.0 dm))]
        (+ (math/sqrt (/ (* 2.0 (* var l)) m)) (/ (* 3.0 (* rng l)) m)))))

  # ── One gauge's running state: Welford's mean and variance, the range ──
  (defn fresh-state [gauge epsilon]
    @{:gauge gauge
      :epsilon epsilon
      :m 0
      :mean 0.0
      :m2 0.0
      :lo (math/inf)
      :hi (math/-inf)
      :half (math/inf)})

  (defn update! [st x]
    (put st :m (+ (get st :m) 1))
    (let [m (get st :m)
          delta (- x (get st :mean))]
      (put st :mean (+ (get st :mean) (/ delta m)))
      # Welford: the second moment uses the UPDATED mean.
      (put st :m2 (+ (get st :m2) (* delta (- x (get st :mean))))))
    (when (< x (get st :lo)) (put st :lo x))
    (when (> x (get st :hi)) (put st :hi x))
    (let [m (get st :m)]
      (put st
           :half (eb-halfwidth m (if (< m 2) 0.0 (/ (get st :m2) (- m 1)))
                               (- (get st :hi) (get st :lo))))))

  (defn all-tight? [states]
    (def @i 0)
    (def @tight true)
    (while (and tight (< i (length states)))
      (let [st (get states i)]
        (when (not (< (get st :half) (get st :epsilon))) (assign tight false)))
      (assign i (+ i 1)))
    tight)

  (defn reading-of [st blocks block]
    "One gauge's state as a reading: the mean is the rate, the half-width its
     interval."
    (let [g (get st :gauge)]
      {:axis (get g :axis)
       :unit (get g :unit)
       :value (get st :mean)
       :half (get st :half)
       :blocks blocks
       :ops (* blocks block)}))

  # ── The sequential estimator ──────────────────────────────────────
  # RUN-BLOCK is (fn [b]) that performs b ops; GAUGES are read before and
  # after every block, each into its own estimator state, and blocks continue
  # until every half-width is under its gauge's EPSILON (bounded by MAXB). One
  # drive, one reading per gauge, all pricing the same operations.
  (defn measure [run-block gauges epsilons block minb maxb]
    "Adaptive empirical-Bernstein rate of RUN-BLOCK's ops on every gauge in
     GAUGES, to the matching EPSILONS. Returns one reading per gauge, in order:
     {:axis :unit :value :half :blocks :ops}. The first block is warmup and is
     discarded — it carries the one-time intercept."
    # Callers are in another compile unit, so no call-site param join can
    # prove these; the allocation-free diverging guards do.
    (when (%not (%int? block)) (error :block-not-int))
    (when (%not (%int? minb)) (error :minb-not-int))
    (when (%not (%int? maxb)) (error :maxb-not-int))
    (run-block block)
    (def n (length gauges))
    # Everything a window needs, made ahead of it: the readers, resolved
    # once, and the two arrays the readings land in.
    (def reads (map (fn [g] (get g :read)) gauges))
    # Mutable arrays the compiler can type, so a `put` inside the window is a
    # store in place. On an array of unknown type it is the general call,
    # which claims a page between the two readings.
    (def befores @[])
    (def afters @[])
    (each g in gauges
      (push befores 0)
      (push afters 0))
    (def @states @[])
    (def @k 0)
    (while (< k n)
      (push states (fresh-state (get gauges k) (get epsilons k)))
      (assign k (+ k 1)))
    (def @blk 0)
    (while (and (%lt blk maxb) (or (%lt blk minb) (not (all-tight? states))))
      (def @i 0)
      (while (%lt i n)
        (put befores i ((get reads i)))
        (assign i (%add i 1)))
      (run-block block)
      (def @j 0)
      (while (%lt j n)
        (put afters j ((get reads j)))
        (assign j (%add j 1)))
      # The window is closed. A reading arrives through a closure value, so
      # it is untyped; the guards prove it before the subtraction.
      (def @g 0)
      (while (< g n)
        (let [before (get befores g)
              after (get afters g)]
          (when (%not (%int? before)) (error :gauge-not-integer))
          (when (%not (%int? after)) (error :gauge-not-integer))
          (update! (get states g) (/ (float (%sub after before)) (float block))))
        (assign g (+ g 1)))
      (assign blk (%add blk 1)))
    (map (fn [st] (reading-of st blk block)) states))

  (defn run-thunk-block [probe b]
    "Run PROBE b times, passing the iteration index — the run-block for a
     direct-loop probe. PROBE is (fn [j]): j varies the input so a body cannot
     constant-fold."
    (when (%not (%int? b)) (error :block-not-int))
    (def @j 0)
    (while (%lt j b)
      (probe j)
      (assign j (%add j 1))))

  (defn stmt-run [thunk]
    "Run THUNK b times as a discarded STATEMENT (non-tail) — the while-loop
     shape a per-call leak needs to surface; a thunk wrapper's return
     convention would reclaim the discarded-statement over-keep on its own."
    (fn [b]
      (when (%not (%int? b)) (error :block-not-int))
      (def @i 0)
      (while (%lt i b)
        (thunk)
        (assign i (%add i 1)))))

  # ── B-invariance: the instrument's own second self-test ───────────
  # A measured rate is a true PER-OP rate only if it is invariant to the block
  # size. A gauge that accumulates a fixed constant per BLOCK reads
  # net/B = rate + C/B, which SHIFTS with B — a confidently wrong number.
  (defn agree? [a b]
    "Do two readings' intervals overlap, so one per-op rate explains both?"
    (not (or (< (+ (get a :value) (get a :half))
                (- (get b :value) (get b :half)))
             (< (+ (get b :value) (get b :half))
                (- (get a :value) (get a :half))))))

  (defn measure-stable [run-block gauges epsilons block minb maxb]
    "Measure at block sizes B and 2B and cross-check B-invariance. Returns the
     finer (2B) readings, each carrying :alt-value (the rate at B) and, when
     the two intervals do not overlap, :void naming the disagreement."
    (when (%not (%int? block)) (error :block-not-int))
    (let [at-b (measure run-block gauges epsilons block minb maxb)
          at-2b (measure run-block gauges epsilons (%mul 2 block) minb maxb)]
      (def @out @[])
      (def @i 0)
      (while (< i (length at-2b))
        (let [a (get at-b i)
              c (get at-2b i)
              with-alt (put c :alt-value (get a :value))]
          (push out
                (if (agree? a c)
                  with-alt
                  (put with-alt
                       :void (string "block-dependent: " (get a :value) " at B="
                                     block " against " (get c :value)
                                     " at 2B — a per-block "
                                     "artifact, not a per-op rate")))))
        (assign i (+ i 1)))
      out))

  {:measure measure
   :measure-stable measure-stable
   :run-thunk-block run-thunk-block
   :stmt-run stmt-run
   :agree? agree?
   :eb-halfwidth eb-halfwidth})
