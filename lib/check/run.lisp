(elle/epoch 15)
# audited: 2026-11-18
## Running a property on every small case and on random ones, and shrinking a failing case to a smallest one.
## docs/stdlib/check.md
## docs/stdlib/check/overview.md
##
## A case is a struct: the `:value` the generator made, `:failed`, the
## `:error` the property raised, and `:choices`, the choices that made the
## value with every trailing 0 removed. A draw past the end of a sequence
## answers 0, so the trailing zeros add nothing to a replay.

(fn [&named source]
  (defn canonical [choices]
    "CHOICES with every trailing 0 removed."
    (var n (length choices))
    (while (and (> n 0) (= 0 (get choices (- n 1)))) (assign n (- n 1)))
    (if (= n (length choices))
      choices
      (let [out @[]]
        (each i in (range n)
          (push out (get choices i)))
        (freeze out))))

  (defn simpler? [a b]
    "Whether the choice sequence A is simpler than B: shorter, or as long and
     smaller at the first choice that differs."
    (or (< (length a) (length b))
        (and (= (length a) (length b)) (= -1 (compare a b)))))

  (defn run-case [g prop src]
    "Make a value of G from SRC and test PROP on it. A case fails when PROP
     returns false or nil, or raises."
    (let* [v (g (src :draw))
           [ok? result] (protect (prop v))
           [choices _] ((src :record))]
      {:value v
       :failed (or (not ok?) (not result))
       :error (if ok? nil result)
       :choices (canonical choices)}))

  # ── the shrinker ──────────────────────────────────────────────────────
  (defn without [choices i]
    "CHOICES with the choice at I deleted."
    (let [out @[]]
      (each j in (range (length choices))
        (unless (= i j) (push out (get choices j))))
      (freeze out)))

  (defn with-choice [choices i c]
    "CHOICES with C in place of the choice at I."
    (let [out (thaw choices)]
      (put out i c)
      (freeze out)))

  (defn shrink [g prop failure]
    "A failing case no simpler than FAILURE: no deletion of one of its
     choices, and no lowering of one choice by 1 or to 0, fails. Each step
     keeps a candidate that fails and is simpler. A replay answers each choice
     no larger than the candidate's, so a candidate that fails is simpler."
    (var best failure)
    (defn attempt [candidate]
      "The case CANDIDATE makes when it fails, or nil."
      (let [c (run-case g prop (source:replay candidate))]
        (when (and (c :failed) (simpler? (c :choices) (best :choices))) c)))
    (var improved true)
    (while improved
      (assign improved false)
      (var i 0)
      (while (< i (length (best :choices)))
        (let [c (attempt (without (best :choices) i))]
          (if c
            (begin
              (assign best c)
              (assign improved true))
            (assign i (+ i 1)))))
      (var k 0)
      (while (< k (length (best :choices)))
        (let [choices (best :choices)
              v (get choices k)]
          (when (> v 0)
            (let [zero (attempt (with-choice choices k 0))]
              (if zero
                (begin
                  (assign best zero)
                  (assign improved true))
                # Choice 0 passes and choice v fails: search between them for
                # the smallest choice that fails.
                (let [@lo 0
                      @hi v
                      @found nil]
                  (while (> (- hi lo) 1)
                    (let [mid (+ lo (bit/shr (- hi lo) 1))
                          c (attempt (with-choice choices k mid))]
                      (if c
                        (begin
                          (assign hi mid)
                          (assign found c))
                        (assign lo mid))))
                  (when found
                    (assign best found)
                    (assign improved true)))))))
        (assign k (+ k 1))))
    best)

  # ── running a property ────────────────────────────────────────────────
  (defn run [g prop &named @depth @trials @seed]
    "Test PROP on every case at DEPTH, then on TRIALS random cases from SEED,
     and shrink the first case that fails. The defaults are depth 4, 100
     trials and seed 0."
    (default depth 4)
    (default trials 100)
    (default seed 0)
    (var cases 0)
    (defn test-case [src]
      (assign cases (+ cases 1))
      (let [c (run-case g prop src)]
        (when (c :failed) c)))
    (var failure (source:walk depth test-case))
    (when (nil? failure)
      (let [next-source (source:random seed)]
        (var k 0)
        (while (and (nil? failure) (< k trials))
          (assign failure (test-case (next-source)))
          (assign k (+ k 1)))))
    (if (nil? failure)
      {:passed true :cases cases}
      (let [s (shrink g prop failure)]
        {:passed false
         :cases cases
         :value (s :value)
         :choices (s :choices)
         :original (failure :value)
         :error (s :error)})))

  (defn check-assert [g prop &named depth trials seed]
    "Run PROP as `run` does. Return true when it holds, and raise
     :failed-assertion naming the shrunk counterexample and the seed when not."
    (let [r (run g prop :depth depth :trials trials :seed seed)]
      (if (r :passed)
        true
        (error {:error :failed-assertion
                :message (string "check: the property fails on " (r :value)
                                 ", from seed " (or seed 0))
                :value (r :value)
                :seed (or seed 0)}))))

  (defn enumerate [g depth]
    "The value of G for every choice sequence whose sum is at most DEPTH, in
     the lexicographic order of the sequences."
    (let [out @[]]
      (source:walk depth
                   (fn [src]
                     (push out (g (src :draw)))
                     nil))
      (freeze out)))

  (defn sample [g &named @seed @count]
    "COUNT values of G drawn at random from SEED. The defaults are seed 0 and
     100 values."
    (default seed 0)
    (default count 100)
    (let [next-source (source:random seed)
          out @[]]
      (repeat count
        (push out (g ((next-source) :draw))))
      (freeze out)))

  {:run run :assert check-assert :enumerate enumerate :sample sample})
