(elle/epoch 18)
# audited: 2026-11-20
## Running a property on every small case and on random ones, and giving up when too many random cases skip.
## docs/stdlib/check.md
## docs/stdlib/check/overview.md

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

  (defn run [g prop &named @depth @trials @seed @skips]
    "Test PROP on every case at DEPTH, then on random cases from SEED until
     TRIALS of them do not skip, and shrink the first case that fails. Give up
     when SKIPS random cases skip. The defaults are depth 4, 100 trials,
     seed 0 and 10 * TRIALS skips."
    (default depth 4)
    (default trials 100)
    (default seed 0)
    (default skips (* 10 trials))
    (var cases 0)
    (var skipped 0)
    (defn test-case [src]
      "The case SRC makes, counted as skipped or as one of the cases."
      (let [c (shrink:run-case g prop src)]
        (if (c :skipped)
          (assign skipped (+ skipped 1))
          (assign cases (+ cases 1)))
        c))
    (var failure
         (source:walk depth
                      (fn [src]
                        (let [c (test-case src)]
                          (when (c :failed-in) c)))))
    (var given-up false)
    (when (nil? failure)
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
        (let [s (shrink:shrink g prop failure)]
          {:passed false
           :cases cases
           :skipped skipped
           :failed-in (s :failed-in)
           :value (s :value)
           :bits (s :bits)
           :choices (s :choices)
           :original (failure :value)
           :error (s :error)})))

  (defn check-assert [g prop &named depth trials seed skips]
    "Run PROP as `run` does. Return true when it holds. When `run` gave up,
     raise its :check-gave-up error. When PROP fails, raise :failed-assertion
     naming the seed, and the shrunk counterexample or what its generator
     raised."
    (let [r (run g prop :depth depth :trials trials :seed seed :skips skips)]
      (cond
        (r :passed) true
        (nil? (r :failed-in)) (error (r :error))
        true
          (error {:error :failed-assertion
                  :message (string (if (= :generator (r :failed-in))
                                     (string "check: the generator raises "
                                       (r :error))
                                     (string "check: the property fails on "
                                       (r :value))) ", from seed " (or seed 0))
                  :value (r :value)
                  :seed (or seed 0)}))))

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
