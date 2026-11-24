(elle/epoch 15)
# audited: 2026-11-18
# std/proof: variables, the round `changed?` ends, and the rules that grow a variable to a fixpoint.
# docs/stdlib/proof.md
#
# The counter-factual: a fixpoint loop that joins every tuple with every
# tuple on each round reaches the same relations, so no result can tell it
# from one that reads only what is new. The last section counts the calls.

(def proof ((import "std/proof")))

(defn tuples [r]
  "A relation's tuples."
  (proof:elements r))

(defn raises-argument-error? [thunk]
  (let [[ok? err] (protect (thunk))]
    (and (not ok?) (= :argument-error (get err :error)))))

# ── variables and the fixpoint ────────────────────────────────────────
(let [it (proof:iteration)]
  (assert (= false (proof:changed? it))
          "an iteration with no variable has not changed")
  (let [v (proof:variable it "v")]
    (assert (= false (proof:changed? it))
            "nor has one whose variable holds nothing")
    (assert (= [] (tuples (proof:complete v)))
            "and that variable completes empty")))

(let [it (proof:iteration)
      v (proof:variable it "v")]
  (proof:extend v [[2 1] [1 2]])
  (proof:insert v (proof:relation [[3 3] [1 2]]))
  (assert (raises-argument-error? (fn [] (proof:complete v)))
          "complete raises while tuples wait to be added")
  (assert (= true (proof:changed? it)) "a round with new tuples has changed")
  (assert (raises-argument-error? (fn [] (proof:complete v)))
          "complete raises while tuples are new")
  (assert (= false (proof:changed? it)) "the next round, with no rule, has not")
  (assert (= [[1 2] [2 1] [3 3]] (tuples (proof:complete v)))
          "complete returns what extend and insert added, as a relation"))

(let [it (proof:iteration)
      v (proof:variable it "v")]
  (proof:insert v (proof:relation []))
  (assert (= false (proof:changed? it)) "an empty relation adds nothing"))

# A tuple a variable already holds is not new a second time.
(let [it (proof:iteration)
      v (proof:variable it "v")]
  (proof:extend v [[1]])
  (assert (= true (proof:changed? it)) "the first [1] is new")
  (proof:extend v [[1]])
  (assert (= false (proof:changed? it)) "the second [1] is not")
  (proof:extend v [[1] [2]])
  (assert (= true (proof:changed? it)) "but [2] beside it is")
  (proof:changed? it)
  (assert (= [[1] [2]] (tuples (proof:complete v)))
          "and the variable holds both"))

(let [it (proof:iteration)
      v (proof:variable it "v" :distinct false)]
  (proof:extend v [[1]])
  (assert (= true (proof:changed? it)) "to an indistinct variable, [1] is new")
  (proof:extend v [[1]])
  (assert (= true (proof:changed? it)) "and new again when it is added again")
  (assert (= false (proof:changed? it)) "until a round adds nothing")
  (assert (= [[1]] (tuples (proof:complete v)))
          "the completed relation still holds one of it"))

# ── the rules ─────────────────────────────────────────────────────────
# reach(a, c) :- edges(a, b), reach(b, c).
(defn reach-of [edge-tuples]
  (let [edges (proof:relation edge-tuples)
        by-target (proof:map edges (fn [[a b]] [b a]))
        it (proof:iteration)
        reach (proof:variable it "reach")]
    (proof:insert reach edges)
    (while (proof:changed? it)
      (proof:from-join reach reach by-target (fn [b c a] [a c])))
    (tuples (proof:complete reach))))

(assert (= [[1 2] [1 3] [1 4] [2 3] [2 4] [3 4]] (reach-of [[1 2] [2 3] [3 4]]))
        "from-join against a relation reaches a chain's closure")
(assert (= [[1 1] [1 2] [2 1] [2 2]] (reach-of [[1 2] [2 1]]))
        "and ends on a cycle")
(assert (= 465 (length (reach-of (map (fn [i] [i (+ i 1)]) (range 30)))))
        "a chain of 30 edges reaches 465 pairs, through many rounds")

# The three examples of datafrog's own documentation.
(let [it (proof:iteration)
      v (proof:variable it "source")]
  (proof:extend v (map (fn [x] [x (+ x 1)]) (range 10)))
  (proof:extend v (map (fn [x] [(+ x 1) x]) (range 10)))
  (while (proof:changed? it)
    (proof:from-join v v v (fn [key val1 val2] [val1 val2])))
  (assert (= 121 (length (tuples (proof:complete v))))
          "a variable joined with itself: all pairs of 0 to 10"))

(let [it (proof:iteration)
      v (proof:variable it "source")
      thirds (proof:relation (filter (fn [x] (= 0 (rem x 3))) (range 10)))]
  (proof:extend v (map (fn [x] [x (+ x 1)]) (range 10)))
  (while (proof:changed? it)
    (proof:from-antijoin v v thirds (fn [key val] [val key])))
  (assert (= 16 (length (tuples (proof:complete v))))
          "from-antijoin: 10 pairs, and the 6 inverses whose key is no multiple of 3"))

(let [it (proof:iteration)
      v (proof:variable it "source")]
  (proof:extend v (map (fn [x] [x x]) (range 10)))
  (while (proof:changed? it)
    (proof:from-map v v
                    (fn [[key val]]
                      (if (even? val)
                        [key (/ val 2)]
                        [key (+ (* 3 val) 1)]))))
  (assert (= 74 (length (tuples (proof:complete v))))
          "from-map: every number that 0 to 9 visit under the Collatz step"))

# A join of two variables, and of a variable with itself as the output.
(let [it (proof:iteration)
      a (proof:variable it "a")
      b (proof:variable it "b")
      out (proof:variable it "out")]
  (proof:extend a [[1 10] [2 20]])
  (proof:extend b [[1 7] [1 8] [3 9]])
  (while (proof:changed? it)
    (proof:from-join out a b (fn [k v1 v2] [k (+ v1 v2)])))
  (assert (= [[1 17] [1 18]] (tuples (proof:complete out)))
          "from-join pairs two variables"))

# ── a rule reads only what is new ─────────────────────────────────────
# Each pair of tuples that share a key meets once, in whichever round the
# later of the two is new.
(let [it (proof:iteration)
      a (proof:variable it "a")
      b (proof:variable it "b")
      out (proof:variable it "out")
      @calls 0
      @rounds 0]
  (proof:extend a [[1 :a] [1 :b]])
  (proof:extend b [[1 :x]])
  (while (proof:changed? it)
    (assign rounds (+ rounds 1))
    (when (= rounds 2)
      (proof:extend b [[1 :y]]))
    (proof:from-join out a b
                     (fn [k v1 v2]
                       (assign calls (+ calls 1))
                       [v1 v2])))
  (assert (= 4 (length (tuples (proof:complete out))))
          "four pairs share the key")
  (assert (= 4 calls) "and the rule met each pair once"))

(let [it (proof:iteration)
      v (proof:variable it "v")
      out (proof:variable it "out")
      @calls 0]
  (proof:extend v [1 2 3])
  (while (proof:changed? it)
    (proof:from-map out v
                    (fn [t]
                      (assign calls (+ calls 1))
                      (* t 10))))
  (assert (= [10 20 30] (tuples (proof:complete out)))
          "from-map maps each tuple")
  (assert (= 3 calls) "once"))

(println "proof-variables: all tests passed")
