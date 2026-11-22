(elle/epoch 18)
# audited: 2026-11-22
# A census row: a fence's hand facts and the gleaned facts of its function, under one judge.
# tools/glean/overview.md

(fn []
  (def proof ((import "std/proof")))
  (def {:ownership ownership :refusals refusals}
    ((import "../../docs/spec/datalog.md")))
  (def {:released-all released-all} ((import "../../docs/spec/capture.md")))

  (defn spelled [name]
    "NAME as the hand facts spell it: a glean symbol is the keyword of its spelling."
    (if (symbol? name) (keyword (string name)) name))

  (defn names [tuples]
    "The value names of TUPLES, the first element of each, as a relation."
    (proof:relation (map (fn [t] (spelled (get t 0))) tuples)))

  (defn judged [facts]
    "[released refused]: the value names released-all and refusals give for FACTS."
    [(names (released-all facts)) (names (refusals facts (ownership facts)))])

  (defn alone [mine theirs]
    "The names of the relation MINE that the relation THEIRS lacks, in order."
    (freeze (proof:elements (proof:antijoin (proof:map mine (fn [n] [n true]))
                              theirs (fn [n _] n)))))

  (defn row [document fence answer]
    "The census row of FENCE, of DOCUMENT, against the function that bears its name in
  ANSWER, glean's answer for the fence's unit."
    (def name (get fence :name))
    (def head {:document document :name name})
    (def found
      (filter (fn [fun] (= (get fun :name) name)) (get answer :functions)))
    (if (empty? found)
      (merge head {:error (string "no function is named " name)})
      (let [fun (first found)
            [ok? both] (protect [(judged (get fence :facts))
                                 (judged (get fun :facts))])]
        (if (not ok?)
          (merge head {:error (get both :message)})
          (let [[[hand-released hand-refused] [glean-released glean-refused]] both]
            (merge head
                   {:marks (get fun :marks)
                    :released {:hand (alone hand-released glean-released)
                               :glean (alone glean-released hand-released)}
                    :refused {:hand (alone hand-refused glean-refused)
                              :glean (alone glean-refused hand-refused)}}))))))

  {:row row})
