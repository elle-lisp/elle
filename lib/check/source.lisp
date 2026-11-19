(elle/epoch 15)
# audited: 2026-11-18
## The sources `draw` reads: random bits from SplitMix64, a walk over every choice sequence up to a size, and a replay of bits.
## docs/stdlib/check/overview.md
##
## A source is a struct. `:draw` answers a draw from the bits the source
## reads, and `:record` returns what the source read so far as a struct:
## `:bits`, every bit read, and `:draws`, one struct per draw holding its
## `:bound`, its `:choice`, and where its bits `:start`, where its
## `:accepted` bits start, and where they `:end`.

(fn []
  (def limit (bit/shl 1 62))

  (defn check-bound [n]
    "Raise unless N is a bound a draw takes: an integer from 1 to 2^62."
    (unless (and (int? n) (<= 1 n) (<= n limit))
      (error {:error :argument-error
              :message (string "draw: expected an integer from 1 to 2^62, got "
                               n)})))

  (defn width [n]
    "How many bits write N less one: 0 for a bound of 1."
    (var k 0)
    (while (< (bit/shl 1 k) n) (assign k (+ k 1)))
    k)

  (defn push-bits [out c k]
    "Push the K bits of C onto OUT, the most significant first."
    (var j (- k 1))
    (while (>= j 0)
      (push out (bit/and (bit/shr c j) 1))
      (assign j (- j 1))))

  (defn recording [next-bit]
    "A source whose draws read bits from NEXT-BIT, a function of the index of
     the bit, and record every bit and every draw. A draw reads the bits of
     its bound less one, and reads as many again while they name the bound or
     more."
    (def bits @[])
    (def draws @[])
    (defn read [k]
      "Read K bits as an integer, the most significant first."
      (var v 0)
      (repeat k
        (let [b (next-bit (length bits))]
          (push bits b)
          (assign v (+ (* 2 v) b))))
      v)
    {:draw (fn [n]
             (check-bound n)
             (let [k (width n)
                   start (length bits)]
               (var at start)
               (var c (read k))
               (while (>= c n)
                 (assign at (length bits))
                 (assign c (read k)))
               (push draws
                     {:bound n
                      :choice c
                      :start start
                      :accepted at
                      :end (length bits)})
               c))
     :record (fn [] {:bits (freeze bits) :draws (freeze draws)})})

  (defn replay [sequence]
    "A source that reads the bits of SEQUENCE in order, then 0."
    (recording (fn [i] (if (< i (length sequence)) (get sequence i) 0))))

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
    "A SplitMix64 stream from SEED, as a function that answers its next bit.
     Each 64-bit output gives 64 bits, the most significant first."
    (var state seed)
    (var word 0)
    (var left 0)
    (fn [_]
      (when (= left 0)
        (assign state (+ state gamma))
        (let [a (* (bit/xor state (shift-right state 30)) mix-a)
              b (* (bit/xor a (shift-right a 27)) mix-b)]
          (assign word (bit/xor b (shift-right b 31))))
        (assign left 64))
      (assign left (- left 1))
      # The lowest bit of the word shifted right by LEFT is bit LEFT of the
      # word, whatever the sign bit copies in above it.
      (bit/and (bit/shr word left) 1)))

  (defn random [seed]
    "A function that makes the source of each next random case. Every source
     it makes reads bits from one SplitMix64 stream, seeded with SEED."
    (let [next-bit (stream seed)]
      (fn [] (recording next-bit))))

  # ── the walk over every small sequence ────────────────────────────────
  (defn successor [record depth]
    "The bits that begin the choice sequence after RECORD's, in lexicographic
     order among those whose sum is at most DEPTH, or nil when none does. The
     last choice that can grow by one grows, and every draw after it reads
     zeros past the end. A walk's draws never reject, so each choice is
     written at its draw's width."
    (let [draws (record :draws)
          sums @[0]]
      (each d in draws
        (push sums (+ (get sums (- (length sums) 1)) (d :choice))))
      (var i (- (length draws) 1))
      (var found nil)
      (while (and (nil? found) (>= i 0))
        (let [d (get draws i)
              c (d :choice)]
          (when (and (< (+ c 1) (d :bound)) (<= (+ (get sums i) c 1) depth))
            (let [prefix @[]]
              (each j in (range i)
                (let [e (get draws j)]
                  (push-bits prefix (e :choice) (width (e :bound)))))
              (push-bits prefix (+ c 1) (width (d :bound)))
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
