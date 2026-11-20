(elle/epoch 18)
# audited: 2026-11-20
## Running a property on every small case and on random ones, and shrinking a failing case to a smallest one.
## docs/stdlib/check.md
## docs/stdlib/check/overview.md
##
## A case is a struct: the `:value` the generator made, `:failed`, the
## `:error` the property raised, `:bits`, every bit its draws read,
## `:choices`, the choice of each draw, and `:draws`, the source's record of
## each draw. A case keeps every bit of every draw: a draw reads its most
## significant bit first, so a case without its trailing zeros would rank
## 512 of a 10-bit draw simpler than 1.

(fn [&named source]
  (defn simpler? [a b]
    "Whether the bits A are simpler than B: fewer, or as many and smaller at
     the first bit that differs."
    (or (< (length a) (length b))
        (and (= (length a) (length b)) (= -1 (compare a b)))))

  (defn run-case [g prop src]
    "Make a value of G from SRC and test PROP on it. A case fails when PROP
     returns false or nil, or raises."
    (let* [v (g (src :draw))
           [ok? result] (protect (prop v))
           record ((src :record))
           choices @[]]
      (each d in (record :draws)
        (push choices (d :choice)))
      {:value v
       :failed (or (not ok?) (not result))
       :error (if ok? nil result)
       :bits (record :bits)
       :choices (freeze choices)
       :draws (record :draws)}))

  # ── the shrinker ──────────────────────────────────────────────────────
  (defn spliced [bits start end middle]
    "BITS with the bits from START up to END replaced by the bits of MIDDLE."
    (let [out @[]]
      (each i in (range start)
        (push out (get bits i)))
      (each b in middle
        (push out b))
      (each i in (range end (length bits))
        (push out (get bits i)))
      (freeze out)))

  (defn bits-of [c k]
    "The K bits of C, the most significant first."
    (let [out @[]]
      (var j (- k 1))
      (while (>= j 0)
        (push out (bit/and (bit/shr c j) 1))
        (assign j (- j 1)))
      (freeze out)))

  (defn deletions [d]
    "The spans of draw D the shrinker tries to delete, as [start end]: every
     bit the draw read, then the bits it rejected."
    (if (< (d :start) (d :accepted))
      [[(d :start) (d :end)] [(d :start) (d :accepted)]]
      [[(d :start) (d :end)]]))

  (defn shrink [g prop failure]
    "A failing case no simpler than FAILURE: no deletion of one draw's bits or
     of the bits it rejected, no deletion of one element of a repeated draw
     with its length written one less, and no lowering of one draw's choice by
     1 or to 0, gives a simpler case that fails. Each step keeps a candidate
     that fails and whose bits are simpler."
    (var best failure)
    (defn attempt [candidate]
      "The case the bits CANDIDATE make when it fails and is simpler, or nil."
      (let [c (run-case g prop (source:replay candidate))]
        (when (and (c :failed) (simpler? (c :bits) (best :bits))) c)))
    (defn lowered [d c]
      "BEST's bits with C written over the accepted bits of draw D."
      (spliced (best :bits) (d :accepted) (d :end)
               (bits-of c (- (d :end) (d :accepted)))))
    (defn element-deleted [d start end]
      "BEST's bits without the element of D's array from START to END, and
       with D's choice, which chose the array's length, written one less."
      (spliced (lowered d (- (d :choice) 1)) start end []))
    (var improved true)
    (while improved
      (assign improved false)
      (let [@i 0]
        (while (< i (length (best :draws)))
          (let [d (get (best :draws) i)]
            (var kept nil)
            (when (> (d :choice) 0)
              (each [start end] in (or (get d :elements) [])
                (when (nil? kept)
                  (assign kept (attempt (element-deleted d start end))))))
            (each [start end] in (deletions d)
              (when (nil? kept)
                (assign kept (attempt (spliced (best :bits) start end [])))))
            (if kept
              (begin
                (assign best kept)
                (assign improved true))
              (assign i (+ i 1)))))
        (var k 0)
        (while (< k (length (best :draws)))
          (let [d (get (best :draws) k)
                v (d :choice)]
            (when (> v 0)
              (let [zero (attempt (lowered d 0))]
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
                            c (attempt (lowered d mid))]
                        (if c
                          (begin
                            (assign hi mid)
                            (assign found c))
                          (assign lo mid))))
                    (when found
                      (assign best found)
                      (assign improved true)))))))
          (assign k (+ k 1)))))
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
         :bits (s :bits)
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
    "COUNT values of G drawn from random bits seeded with SEED. The defaults
     are seed 0 and 100 values."
    (default seed 0)
    (default count 100)
    (let [next-source (source:random seed)
          out @[]]
      (repeat count
        (push out (g ((next-source) :draw))))
      (freeze out)))

  {:run run :assert check-assert :enumerate enumerate :sample sample})
