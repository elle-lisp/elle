(elle/epoch 18)
# audited: 2026-11-20
## A sequence of bits: the width of a bound, writing and reading a choice, splicing, and which of two sequences is simpler.
## docs/stdlib/check.md
## docs/stdlib/check/overview.md
##
## A sequence is an array of the integers 0 and 1. A choice is written in the
## bits of its draw's width, the most significant first.

(fn []
  (defn width [n]
    "How many bits write N less one: 0 for a bound of 1."
    (var k 0)
    (while (< (bit/shl 1 k) n) (assign k (+ k 1)))
    k)

  (defn write [out c k]
    "Push the K bits of C onto OUT, the most significant first. Return OUT."
    (var j (- k 1))
    (while (>= j 0)
      (push out (bit/and (bit/shr c j) 1))
      (assign j (- j 1)))
    out)

  (defn encode [c k]
    "The K bits of C, the most significant first."
    (freeze (write @[] c k)))

  (defn read [next k]
    "K bits as an integer, the most significant first. NEXT is a function of
     no arguments that answers each next bit."
    (var v 0)
    (repeat k
      (assign v (+ (* 2 v) (next))))
    v)

  (defn simpler? [a b]
    "Whether the bits A are simpler than B: fewer, or as many and smaller at
     the first bit that differs."
    (or (< (length a) (length b))
        (and (= (length a) (length b)) (= -1 (compare a b)))))

  (defn spliced [bits start end middle]
    "BITS with the bits from START up to END replaced by the bits of MIDDLE."
    (freeze (concat (slice bits 0 start) middle (slice bits end))))

  (defn bit-array? [x]
    "Whether X is an array of the integers 0 and 1."
    (and (array? x) (all? (fn [b] (or (identical? b 0) (identical? b 1))) x)))

  {:width width
   :write write
   :encode encode
   :read read
   :simpler? simpler?
   :spliced spliced
   :bit-array? bit-array?})
