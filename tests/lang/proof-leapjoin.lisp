(elle/epoch 15)
# audited: 2026-11-18
# std/proof: the leapjoin over a relation and over a variable, and each of its six leapers.
# docs/stdlib/proof.md
#
# The counter-factual: a leaper that filters is given no value to propose, so
# a leapjoin of filters alone has nothing to intersect. It raises, or it
# proposes once for each tuple it keeps. The last section pins which.

(def proof ((import "std/proof")))

(defn tuples [r]
  "A relation's tuples."
  (proof:elements r))

(defn raises-argument-error? [thunk]
  (let [[ok? err] (protect (thunk))]
    (and (not ok?) (= :argument-error (get err :error)))))

# ── leapjoin over a relation ──────────────────────────────────────────
(defn self [t]
  t)

(def ones (proof:relation [[1 1] [1 2] [1 3] [2 1]]))
(def twos (proof:relation [[1 2] [1 3] [1 4] [2 5]]))
(def source (proof:relation [1 2 3]))

(defn leap [leapers]
  (tuples (proof:leapjoin source leapers (fn [t v] [t v]))))

(assert (= [[1 1] [1 2] [1 3] [2 1]] (leap (proof:extend-with ones self)))
        "extend-with proposes each value its relation holds for the key")
(assert (= [[1 2] [1 3]]
           (leap [(proof:extend-with ones self) (proof:extend-with twos self)]))
        "two extend-with leapers keep the values both hold")
(assert (= [[1 2] [1 3]]
           (leap [(proof:extend-with twos self) (proof:extend-with ones self)]))
        "in either order")
(assert (= [[1 1] [2 1]]
           (leap [(proof:extend-with ones self) (proof:extend-anti twos self)]))
        "extend-anti removes the values its relation holds for the key")
(assert (= [[1 1] [1 2] [1 3]]
           (leap [(proof:extend-with ones self)
                  (proof:filter-with (proof:relation [[1 :ok]]) (fn [t] [t :ok]))]))
        "filter-with keeps a tuple whose pair its relation holds")
(assert (= [[2 1]]
           (leap [(proof:extend-with ones self)
                  (proof:filter-anti (proof:relation [[1 :ok]]) (fn [t] [t :ok]))]))
        "filter-anti keeps a tuple whose pair its relation lacks")
(assert (= [[2 1]]
           (leap [(proof:extend-with ones self)
                  (proof:prefix-filter (fn [t] (> t 1)))]))
        "prefix-filter keeps a tuple its predicate accepts")
(assert (= [[1 2] [1 3]]
           (leap [(proof:extend-with ones self)
                  (proof:value-filter (fn [t v] (> v t)))]))
        "value-filter keeps a value its predicate accepts")
(assert (= []
           (leap [(proof:extend-with ones self)
                  (proof:prefix-filter (fn [t] false))]))
        "a leapjoin that keeps no tuple is empty")

# A leaper that filters, standing alone, proposes nil once for a kept tuple.
(def pairs-a (proof:relation [[1 2] [3 4] [5 6]]))
(def pairs-b (proof:relation [[3 4] [5 6] [7 8]]))

(defn alone [leapers]
  (tuples (proof:leapjoin pairs-a leapers (fn [t v] [t v]))))

(assert (= [[[3 4] nil] [[5 6] nil]] (alone (proof:filter-with pairs-b self)))
        "filter-with alone is an intersection, and proposes nil")
(assert (= [[[1 2] nil]] (alone (proof:filter-anti pairs-b self)))
        "filter-anti alone is a difference")
(assert (= [[[3 4] nil] [[5 6] nil]]
           (alone (proof:prefix-filter (fn [[a b]] (> a 2)))))
        "prefix-filter alone filters its source")
(assert (= [[[3 4] nil] [[5 6] nil]] (alone [(proof:filter-with pairs-b self)]))
        "an array of one leaper is that leaper alone")

# One of the leapers must propose values.
(each [name leapers] in [["extend-anti alone" (proof:extend-anti ones self)]
                         ["value-filter alone"
                          (proof:value-filter (fn [t v] true))]
                         ["a filter-with beside a value-filter"
                          [(proof:filter-with pairs-b self)
                           (proof:value-filter (fn [t v] true))]]
                         ["a prefix-filter beside an extend-anti"
                          [(proof:prefix-filter (fn [t] true))
                           (proof:extend-anti ones self)]] ["no leaper" []]]
  (assert (raises-argument-error? (fn []
                                    (proof:leapjoin source leapers
                                      (fn [t v] [t v]))))
          (string "a leapjoin with no proposer raises :argument-error: " name))
  (assert (raises-argument-error? (fn []
                                    (proof:leapjoin (proof:relation []) leapers
                                      (fn [t v] [t v]))))
          (string "also over an empty source: " name)))

# ── leapjoin over a variable ──────────────────────────────────────────
# datafrog's leapjoin_from_extend: a leapjoin reads the tuples an extend
# added before the first round.
(let [doubles (proof:relation (map (fn [i] [i (* 2 i)]) (range 10)))
      it (proof:iteration)
      v (proof:variable it "v")]
  (proof:extend v [[2 2]])
  (while (proof:changed? it)
    (proof:from-leapjoin v v (proof:extend-with doubles (fn [[i _]] i))
                         (fn [[i _] j] [i j])))
  (assert (= [[2 2] [2 4]] (tuples (proof:complete v)))
          "from-leapjoin matches the tuples of the first round"))

(let [it (proof:iteration)
      numbers (proof:variable it "numbers")
      out (proof:variable it "out")]
  (proof:extend numbers [1 2 3])
  (while (proof:changed? it)
    (proof:from-leapjoin out numbers
                         [(proof:extend-with ones self)
                          (proof:extend-anti twos self)] (fn [t v] [t v])))
  (assert (= [[1 1] [2 1]] (tuples (proof:complete out)))
          "from-leapjoin takes an array of leapers"))

(println "proof-leapjoin: all tests passed")
