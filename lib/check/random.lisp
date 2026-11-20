(elle/epoch 18)
# audited: 2026-11-20
## The SplitMix64 stream, and the random sources it feeds, which answer a draw's edge one time in 8.
## docs/stdlib/check.md
## docs/stdlib/check/overview.md

(fn [&named bits source]
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

  (defn edge-decider [next-bit]
    "A decider that answers one of a draw's edges one time in 8, each as often
     as the others, and nil otherwise, or when the draw names none. It reads
     its bits from NEXT-BIT and records none of them, so a replay needs no
     edges."
    (defn next []
      (next-bit nil))
    (fn [_ edges]
      (when (and edges (not (empty? edges)) (= 0 (bits:read next 3)))
        (let [n (length edges)
              k (bits:width n)]
          (var i (bits:read next k))
          (while (>= i n) (assign i (bits:read next k)))
          (get edges i)))))

  (defn sources [seed]
    "A function that makes the source of each next random case, whatever
     case came before. Every source it makes reads bits from one SplitMix64
     stream, seeded with SEED, and decides from that stream on edges."
    (let [next-bit (stream seed)
          decide (edge-decider next-bit)]
      (fn [&opt _] (source:recording next-bit decide))))

  {:sources sources})
