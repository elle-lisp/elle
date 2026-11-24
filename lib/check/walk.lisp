(elle/epoch 18)
# audited: 2026-11-20
## The depth-first walk over every choice sequence whose sum is at most a depth, as the source of each next case.
## docs/stdlib/check.md
## docs/stdlib/check/overview.md

(fn [&named bits source]
  (defn successor [record depth]
    "The bits that begin the choice sequence after RECORD's, in lexicographic
     order among those whose sum is at most DEPTH, or nil when none does. The
     last choice that can grow by one grows, and every draw after it reads
     zeros past the end. A walk's draws never reject, so each choice is
     written at its draw's width."
    (let [choices (record :choices)
          bounds (record :bounds)
          sums @[0]]
      (each c in choices
        (push sums (+ (get sums (- (length sums) 1)) c)))
      (var i (- (length choices) 1))
      (var found nil)
      (while (and (nil? found) (>= i 0))
        (let [c (get choices i)
              bound (get bounds i)]
          (when (and (< (+ c 1) bound) (<= (+ (get sums i) c 1) depth))
            (let [prefix @[]]
              (each j in (range i)
                (bits:write prefix (get choices j) (bits:width (get bounds j))))
              (bits:write prefix (+ c 1) (bits:width bound))
              (assign found (freeze prefix)))))
        (assign i (- i 1)))
      found))

  (defn sources [depth]
    "A function of the case before that answers the source of the next choice
     sequence whose sum is at most DEPTH: the empty sequence first, then each
     in lexicographic order, and nil once none is left. A draw past the end
     of a sequence answers 0."
    (fn [previous]
      (if (nil? previous)
        (source:replay [])
        (let [next (successor (previous :record) depth)]
          (when next (source:replay next))))))

  {:sources sources})
