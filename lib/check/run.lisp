(elle/epoch 18)
# audited: 2026-11-20
## Running a property on every small case and random ones, or on one replayed case, and giving up when too many random cases skip.
## docs/stdlib/check.md
## docs/stdlib/check/overview.md
## docs/stdlib/check/functions.md

(fn [&named source shrink]
  (defn gave-up [what n]
    "The :check-gave-up error of WHAT, which gave up when N cases skipped."
    {:error :check-gave-up
     :message (string "check: " n " cases skipped, so " what " gave up")})

  (defn made [g src]
    "The value G makes from SRC as [true v], or [false nil] when the case
     skips. Any other error G raises propagates."
    (let [[ok? v] (protect (g (src :draw)))]
      (cond
        ok? [true v]
        (shrink:skip? v) [false nil]
        true (error v))))

  (defn argument-error [message]
    (error {:error :argument-error :message message}))

  (defn bit-array? [bits]
    "Whether BITS is an array of the integers 0 and 1."
    (and (array? bits)
         (all? (fn [b] (or (identical? b 0) (identical? b 1))) bits)))

  (defn run [g prop &named @depth @trials @seed @skips @shrinks bits]
    "Test PROP on every case at DEPTH, then on random cases from SEED until
     TRIALS of them do not skip, and shrink the first case that fails, running
     at most SHRINKS candidates. Give up when SKIPS random cases skip. Given
     BITS, test the one case a replay of BITS makes instead. The defaults are
     depth 4, 100 trials, seed 0, 10 * TRIALS skips and 1000 shrinks."
    (default depth 4)
    (default trials 100)
    (default seed 0)
    (default skips (* 10 trials))
    (default shrinks 1000)
    (unless (or (nil? bits) (bit-array? bits))
      (argument-error (string "run: expected :bits to be an array of 0 and 1, "
                              "got " bits)))
    (unless (and (int? shrinks) (>= shrinks 0))
      (argument-error (string "run: expected :shrinks to be an integer of 0 or "
                              "more, got " shrinks)))
    (var cases 0)
    (var skipped 0)
    (defn test-case [src]
      "The case SRC makes, counted as skipped or as one of the cases."
      (let [c (shrink:run-case g prop src)]
        (if (c :skipped)
          (assign skipped (+ skipped 1))
          (assign cases (+ cases 1)))
        c))
    (defn failing [src]
      "The case SRC makes when it fails, or nil."
      (let [c (test-case src)]
        (when (c :failed-in) c)))
    (var failure
         (if (nil? bits)
           (source:walk depth failing)
           (failing (source:replay bits))))
    (var given-up false)
    (when (and (nil? failure) (nil? bits))
      (let [next-source (source:random seed)
            @held 0
            @random-skips 0]
        (while (and (nil? failure) (not given-up) (< held trials))
          (let [c (test-case (next-source))]
            (cond
              (c :skipped)
                (begin
                  (assign random-skips (+ random-skips 1))
                  (assign given-up (>= random-skips skips)))
              (c :failed-in) (assign failure c)
              true (assign held (+ held 1)))))))
    (cond
      given-up
        {:passed false
         :cases cases
         :skipped skipped
         :error (gave-up "run" skips)}
      (nil? failure) {:passed true :cases cases :skipped skipped}
      true
        (let [s (shrink:shrink g prop failure shrinks)]
          {:passed false
           :cases cases
           :skipped skipped
           :failed-in (s :failed-in)
           :value (s :value)
           :bits (s :bits)
           :choices (s :choices)
           :minimal (s :minimal)
           :original (failure :value)
           :error (s :error)
           :calls (s :calls)})))

  (defn check-assert [g prop &named depth trials seed skips shrinks bits]
    "Run PROP as `run` does. Return true when it holds. When `run` gave up,
     raise its :check-gave-up error. When PROP fails, raise :failed-assertion
     naming the shrunk counterexample or what its generator raised, the calls
     of the functions it made, and the seed or the replayed bits. The error's
     :bits holds the shrunk case's."
    (let [r (run g prop :depth depth :trials trials :seed seed :skips skips
                 :shrinks shrinks :bits bits)]
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
                whence (if (nil? bits)
                         (string ", from seed " (or seed 0))
                         ", from the replayed bits")]
            (error {:error :failed-assertion
                    :message (string what calls whence)
                    :value (r :value)
                    :seed (or seed 0)
                    :bits (r :bits)})))))

  (defn enumerate [g depth]
    "The value of G for every choice sequence whose sum is at most DEPTH, in
     the lexicographic order of the sequences, but for each case that skips."
    (let [out @[]]
      (source:walk depth
                   (fn [src]
                     (let [[kept? v] (made g src)]
                       (when kept? (push out v)))
                     nil))
      (freeze out)))

  (defn sample [g &named @seed @count]
    "COUNT values of G drawn from random bits seeded with SEED, drawing past
     each case that skips. Raise :check-gave-up when 10 * COUNT cases skip.
     The defaults are seed 0 and 100 values."
    (default seed 0)
    (default count 100)
    (let [next-source (source:random seed)
          out @[]
          @skipped 0]
      (while (< (length out) count)
        (let [[kept? v] (made g (next-source))]
          (if kept?
            (push out v)
            (begin
              (assign skipped (+ skipped 1))
              (when (>= skipped (* 10 count)) (error (gave-up "sample" skipped)))))))
      (freeze out)))

  {:run run :assert check-assert :enumerate enumerate :sample sample})
