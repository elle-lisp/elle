(elle/epoch 18)
# audited: 2026-11-20
## The search over phases of sources, and `run`, `assert`, `enumerate` and `sample` above it, each a search of one or two phases.
## docs/stdlib/check.md
## docs/stdlib/check/overview.md
## docs/stdlib/check/functions.md

(fn [&named bits errors source random walk case shrink]
  # ── options ───────────────────────────────────────────────────────────
  (def defaults {:depth 4 :trials 100 :seed 0 :shrinks 1000})
  (def known |:depth :trials :seed :skips :shrinks :bits|)

  (defn options [what given]
    "The options GIVEN to WHAT, each one GIVEN lacks or holds as nil at its
     default. :skips defaults to 10 * :trials, and :bits to nil. Raise
     :argument-error for an option WHAT does not take, or a bad value."
    (each k in (keys given)
      (unless (contains? known k)
        (errors:argument-error (string what ": unknown option " k
                                       ", valid options are " known))))
    (let* [present (from-pairs (filter (fn [[_ v]] (not (nil? v))) (pairs given)))
           o (merge defaults present)
           o (if (nil? (o :skips))
               (put o :skips (* 10 (o :trials)))
               o)]
      (unless (or (nil? (o :bits)) (bits:bit-array? (o :bits)))
        (errors:argument-error (string what
                                       ": expected :bits to be an array of 0 "
                                       "and 1, got " (o :bits))))
      (unless (and (int? (o :shrinks)) (>= (o :shrinks) 0))
        (errors:argument-error (string what
                                       ": expected :shrinks to be an integer "
                                       "of 0 or more, got " (o :shrinks))))
      o))

  # ── the search ────────────────────────────────────────────────────────
  (defn phase [next &named trials skips]
    "A phase of a search. NEXT is a function of the case before, nil for the
     first, that answers the source of the next case, or nil when the phase
     ends. The phase also ends once TRIALS cases pass, and gives up once
     SKIPS cases skip. Nil sets no bound."
    {:next next :trials trials :skips skips})

  (defn replayed [sequence]
    "The phase of the one case a replay of the bits SEQUENCE makes."
    (phase (fn [previous] (when (nil? previous) (source:replay sequence)))))

  (defn search [g prop phases]
    "Test PROP on the cases of each phase in turn, until a case fails or a
     phase gives up. A struct: the failing case as :failure, how many :cases
     passed or failed, how many :skipped, and as :gave-up the skips of the
     phase that gave up."
    (var cases 0)
    (var skipped 0)
    (var failure nil)
    (var gave-up nil)
    (each p in phases
      (let [@previous nil
            @held 0
            @skips 0
            @done false]
        (while (and (not done) (nil? failure) (nil? gave-up))
          (let [src (unless (and (p :trials) (>= held (p :trials)))
                      ((p :next) previous))]
            (if (nil? src)
              (assign done true)
              (let [c (case:run-case g prop src)]
                (assign previous c)
                (if (c :skipped)
                  (begin
                    (assign skipped (+ skipped 1))
                    (assign skips (+ skips 1))
                    (when (and (p :skips) (>= skips (p :skips)))
                      (assign gave-up skips)))
                  (begin
                    (assign cases (+ cases 1))
                    (if (c :failed-in)
                      (assign failure c)
                      (assign held (+ held 1)))))))))))
    {:failure failure :cases cases :skipped skipped :gave-up gave-up})

  # ── run and assert ────────────────────────────────────────────────────
  (def shrunk-keys [:failed-in :value :bits :choices :minimal :error :calls])

  (defn run-with [g prop o]
    "Run PROP on G's cases under the options O, as `run` does."
    (let [found (search g prop
                        (if (nil? (o :bits))
                          [(phase (walk:sources (o :depth)))
                           (phase (random:sources (o :seed)) :trials (o :trials)
                                  :skips (o :skips))]
                          [(replayed (o :bits))]))
          tally {:cases (found :cases) :skipped (found :skipped)}
          failure (found :failure)]
      (cond
        (found :gave-up)
          (merge tally {:passed false :error (errors:gave-up "run" (o :skips))})
        (nil? failure) (put tally :passed true)
        true
          (let [s (shrink:shrink g prop failure (o :shrinks))]
            (merge (from-pairs (map (fn [k] [k (get s k)]) shrunk-keys))
                   (merge tally {:passed false :original (failure :value)}))))))

  (defn run [g prop &keys given]
    "Test PROP on every case at :depth, then on random cases from :seed until
     :trials of them do not skip, and shrink the first case that fails,
     running at most :shrinks candidates. Give up when :skips random cases
     skip. Given :bits, test the one case a replay of :bits makes instead.
     The defaults are depth 4, 100 trials, seed 0, 10 * trials skips and 1000
     shrinks."
    (run-with g prop (options "run" given)))

  (defn check-assert [g prop &keys given]
    "Run PROP as `run` does. Return true when it holds. When `run` gave up,
     raise its :check-gave-up error. When PROP fails, raise :failed-assertion
     naming the shrunk counterexample or what its generator raised, the calls
     of the functions it made, and the seed or the replayed bits. The error's
     :bits holds the shrunk case's."
    (let [o (options "assert" given)
          r (run-with g prop o)]
      (cond
        (r :passed) true
        (nil? (r :failed-in)) (error (r :error))
        true
          (let [what (if (= :generator (r :failed-in))
                       (string "check: the generator raises " (r :error))
                       (string "check: the property fails on " (r :value)))
                calls (if (empty? (r :calls))
                        ""
                        (string ", and its functions answered " (r :calls)))
                whence (if (nil? (o :bits))
                         (string ", from seed " (o :seed))
                         ", from the replayed bits")]
            (error (errors:failed-assertion (string what calls whence) r
                     (o :seed)))))))

  # ── enumerate and sample ──────────────────────────────────────────────
  (defn values-of [g phases]
    "The value of each case of G over PHASES, but for each case that skips,
     and the skips of a phase that gave up, or nil. Raise what G raises."
    (let [out @[]
          found (search g
                        (fn [v]
                          (push out v)
                          true) phases)]
      (when (found :failure)
        (error ((found :failure) :error)))
      [(freeze out) (found :gave-up)]))

  (defn enumerate [g depth]
    "The value of G for every choice sequence whose sum is at most DEPTH, in
     the lexicographic order of the sequences, but for each case that skips."
    (get (values-of g [(phase (walk:sources depth))]) 0))

  (defn sample [g &named @seed @count]
    "COUNT values of G drawn from random bits seeded with SEED, drawing past
     each case that skips. Raise :check-gave-up when 10 * COUNT cases skip.
     The defaults are seed 0 and 100 values."
    (default seed 0)
    (default count 100)
    (let [[out gave-up] (values-of g
                                   [(phase (random:sources seed) :trials count
                                      :skips (* 10 count))])]
      (when gave-up (error (errors:gave-up "sample" gave-up)))
      out))

  {:run run :assert check-assert :enumerate enumerate :sample sample})
