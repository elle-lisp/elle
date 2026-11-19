(elle/epoch 15)
# audited: 2026-11-18
# std/proof against generated inputs: the five properties of datafrog's own
# suite, on std/check, and each engine result against a direct computation.
# docs/stdlib/proof.md
#
# The counter-factual: datafrog's properties compare one path through the
# engine with another, so a defect both paths share passes all five. Each
# result here is also compared with a search or a loop that uses no relation.

(def proof ((import "std/proof")))
(def check ((import "std/check")))

# datafrog draws up to 500 pairs of 0 to 99. The pairs here are fewer and
# their range is narrower, so a case is small enough to run by the hundred
# and the keys of a join still collide.
(def pairs
  (check:array-of (check:tuple (check:int 0 11) (check:int 0 11)) 30 :min 1))
(def two-inputs (check:tuple pairs pairs))

(defn distinct-sorted [xs]
  "The tuples of xs, sorted, one of each: what a relation of xs must hold."
  (sort (->array (apply set xs))))

(defn holds? [gen prop]
  (check:assert gen prop :depth 3 :trials 40 :seed 2026))

# ── three ways to compute reachability ────────────────────────────────
(defn inverted [edges]
  (map (fn [[n1 n2]] [n2 n1]) (proof:elements edges)))

(defn reachable-with-var-join [edge-tuples]
  (let [edges (proof:relation edge-tuples)
        it (proof:iteration)
        by-successor (proof:variable it "edges-invert")
        reachable (proof:variable it "reachable")]
    (proof:extend by-successor (inverted edges))
    (proof:insert reachable edges)
    (while (proof:changed? it)
      # reachable(n1, n3) :- edges(n1, n2), reachable(n2, n3).
      (proof:from-join reachable reachable by-successor (fn [_ n3 n1] [n1 n3])))
    (proof:elements (proof:complete reachable))))

(defn reachable-with-relation-join [edge-tuples]
  (let [edges (proof:relation edge-tuples)
        by-successor (proof:relation (inverted edges))
        it (proof:iteration)
        reachable (proof:variable it "reachable")]
    (proof:insert reachable edges)
    (while (proof:changed? it)
      (proof:from-join reachable reachable by-successor (fn [_ n3 n1] [n1 n3])))
    (proof:elements (proof:complete reachable))))

(defn reachable-with-leapfrog [edge-tuples]
  (let [edges (proof:relation edge-tuples)
        by-successor (proof:relation (inverted edges))
        it (proof:iteration)
        reachable (proof:variable it "reachable")]
    (proof:insert reachable edges)
    (while (proof:changed? it)
      (proof:from-leapjoin reachable reachable
                           (proof:extend-with by-successor (fn [[n2 _]] n2))
                           (fn [[_ n3] n1] [n1 n3])))
    (proof:elements (proof:complete reachable))))

(defn reachable-by-search [edge-tuples]
  "Every [a c] with a path of one edge or more from a to c, with no relation."
  (let [edges (thaw edge-tuples)
        found @||
        @grew true]
    (each e in edges
      (add found (freeze e)))
    (while grew
      (assign grew false)
      (each [a b] in (->array found)
        (each [b2 c] in edges
          (when (and (= b b2) (not (has? found [a c])))
            (add found [a c])
            (assign grew true)))))
    (sort (->array found))))

(assert (holds? pairs
                (fn [edges]
                  (= (reachable-with-var-join edges)
                     (reachable-with-leapfrog edges))))
        "reachable: the leapjoin agrees with the join of two variables")
(assert (holds? pairs
                (fn [edges]
                  (= (reachable-with-var-join edges)
                     (reachable-with-relation-join edges))))
        "reachable: a join against a relation agrees with a join of two variables")
(assert (holds? pairs
                (fn [edges]
                  (= (reachable-with-var-join edges) (reachable-by-search edges))))
        "reachable: the fixpoint agrees with a search")

# ── a join whose rule sums its values ─────────────────────────────────
(defn summed [k v1 v2]
  [k (+ (* v1 100) v2)])

(defn sum-join-via-var [xs ys]
  (let [it (proof:iteration)
        input1 (proof:variable it "input1")
        input2 (proof:variable it "input2")
        output (proof:variable it "output")]
    (proof:extend input1 xs)
    (proof:extend input2 ys)
    (while (proof:changed? it)
      # output(k, v1 * 100 + v2) :- input1(k, v1), input2(k, v2).
      (proof:from-join output input1 input2 summed))
    (proof:elements (proof:complete output))))

(defn sum-join-via-relation [xs ys]
  (proof:elements (proof:join (proof:relation xs) (proof:relation ys) summed)))

(defn sum-join-by-loops [xs ys]
  (let [out @[]]
    (each [k1 v1] in xs
      (each [k2 v2] in ys
        (when (= k1 k2) (push out (summed k1 v1 v2)))))
    (distinct-sorted out)))

(assert (holds? two-inputs
                (fn [[xs ys]]
                  (= (sum-join-via-var xs ys) (sum-join-via-relation xs ys))))
        "join: two variables agree with two relations")
(assert (holds? two-inputs
                (fn [[xs ys]]
                  (= (sum-join-via-relation xs ys) (sum-join-by-loops xs ys))))
        "join: two relations agree with two loops")

# ── a filter leaper on its own ────────────────────────────────────────
(defn self [t]
  t)

(defn kept [t _]
  t)

(assert (holds? two-inputs
                (fn [[xs ys]]
                  (let [in-ys (apply set ys)]
                    (= (proof:elements (proof:leapjoin (proof:relation xs)
                                         (proof:filter-with (proof:relation ys)
                                           self) kept))
                       (filter (fn [t] (has? in-ys t)) (distinct-sorted xs))))))
        "filter-with on its own is an intersection")
(assert (holds? two-inputs
                (fn [[xs ys]]
                  (let [in-ys (apply set ys)]
                    (= (proof:elements (proof:leapjoin (proof:relation xs)
                                         (proof:filter-anti (proof:relation ys)
                                           self) kept))
                       (filter (fn [t] (not (has? in-ys t)))
                               (distinct-sorted xs))))))
        "filter-anti on its own is a difference")

# ── a relation, a merge and an antijoin against direct computation ────
(assert (holds? pairs
                (fn [xs]
                  (= (proof:elements (proof:relation xs)) (distinct-sorted xs))))
        "a relation holds its tuples sorted, one of each")
(assert (holds? two-inputs
                (fn [[xs ys]]
                  (= (proof:elements (proof:merge (proof:relation xs)
                                       (proof:relation ys)))
                     (distinct-sorted (concat xs ys)))))
        "a merge is the relation of both inputs")
(assert (holds? two-inputs
                (fn [[xs ys]]
                  (let [keys (apply set (map first ys))]
                    (= (proof:elements (proof:antijoin (proof:relation xs)
                                         (proof:relation (map first ys))
                                         (fn [k v] [k v])))
                       (filter (fn [[k v]] (not (has? keys k)))
                               (distinct-sorted xs))))))
        "an antijoin keeps the tuples whose key is absent")

(println "proof-properties: all tests passed")
