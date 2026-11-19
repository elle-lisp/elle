(elle/epoch 15)
# audited: 2026-11-18
## The sources `draw` reads: a seeded SplitMix64 stream, a walk over every choice sequence up to a size, and a replay.
## docs/stdlib/check/overview.md
##
## A source is a struct. `:draw` answers a draw and records it, and `:record`
## returns what the source answered so far as `[choices bounds]`: the choice
## of each draw, and the bound it was drawn under.

(fn []
  (def limit (bit/shl 1 62))

  (defn check-bound [n]
    "Raise unless N is a bound a draw takes: an integer from 1 to 2^62."
    (unless (and (int? n) (<= 1 n) (<= n limit))
      (error {:error :argument-error
              :message (string "draw: expected an integer from 1 to 2^62, got "
                               n)})))

  (defn recording [answer]
    "A source whose draw asks ANSWER, a function of the bound and the index of
     the draw, for each choice, and records the choice and its bound."
    (def choices @[])
    (def bounds @[])
    {:draw (fn [n]
             (check-bound n)
             (let [c (answer n (length choices))]
               (push choices c)
               (push bounds n)
               c))
     :record (fn [] [(freeze choices) (freeze bounds)])})

  (defn replay [sequence]
    "A source that answers the choices of SEQUENCE in order, then 0. A choice
     at or past its draw's bound answers the bound less one."
    (recording (fn [n i]
                 (if (< i (length sequence)) (min (get sequence i) (- n 1)) 0))))

  # ── the SplitMix64 stream ─────────────────────────────────────────────
  # The constants are the unsigned 64-bit ones of SplitMix64, written as the
  # signed integers with the same bits: 0x9E3779B97F4A7C15, 0xBF58476D1CE4E5B9
  # and 0x94D049BB133111EB. Integers wrap at 64 bits, as the algorithm needs.
  (def gamma -7046029254386353131)
  (def mix-a -4658895280553007687)
  (def mix-b -7723592293110705685)

  (defn shift-right [x k]
    "X shifted right by K bits, with zeros in from the top. `bit/shr` copies
     the sign bit, so the result is masked to the 64 - K bits that remain."
    (bit/and (bit/shr x k) (- (bit/shl 1 (- 64 k)) 1)))

  (defn stream [seed]
    "A SplitMix64 stream from SEED, as a function of a bound and an index that
     answers an integer from 0 to the bound less one."
    (var state seed)
    (fn [n _]
      (assign state (+ state gamma))
      (let [a (* (bit/xor state (shift-right state 30)) mix-a)
            b (* (bit/xor a (shift-right a 27)) mix-b)
            z (bit/xor b (shift-right b 31))]
        (rem (shift-right z 2) n))))

  (defn random [seed]
    "A function that makes the source of each next random case. Every source
     it makes draws from one SplitMix64 stream, seeded with SEED."
    (let [answer (stream seed)]
      (fn [] (recording answer))))

  # ── the walk over every small sequence ────────────────────────────────
  (defn successor [record depth]
    "The prefix of the choice sequence that follows RECORD in lexicographic
     order among those whose sum is at most DEPTH, or nil when none does. The
     last choice that can grow by one grows, and every draw after it answers 0."
    (let [[choices bounds] record
          sums @[0]]
      (each c in choices
        (push sums (+ (get sums (- (length sums) 1)) c)))
      (var i (- (length choices) 1))
      (var found nil)
      (while (and (nil? found) (>= i 0))
        (let [c (get choices i)]
          (when (and (< (+ c 1) (get bounds i)) (<= (+ (get sums i) c 1) depth))
            (let [prefix @[]]
              (each j in (range i)
                (push prefix (get choices j)))
              (push prefix (+ c 1))
              (assign found (freeze prefix)))))
        (assign i (- i 1)))
      found))

  (defn walk [depth visit]
    "Call VISIT with a source for every choice sequence whose sum is at most
     DEPTH, in lexicographic order. A draw past the end of a sequence answers 0.
     Stop at the first value VISIT returns that is not nil, and return it."
    (var prefix [])
    (var result nil)
    (while (not (nil? prefix))
      (let [src (replay prefix)
            v (visit src)]
        (if (nil? v)
          (assign prefix (successor ((src :record)) depth))
          (begin
            (assign result v)
            (assign prefix nil)))))
    result)

  {:replay replay :random random :walk walk})
