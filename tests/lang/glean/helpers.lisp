(elle/epoch 18)
# audited: 2026-11-21
# What glean's test files share: builders for a unit in the data form, and the check of a function's facts.
# tools/glean/overview.md
# tests/lang/overview.md

(defn function [fields]
  "A function struct of the data form: FIELDS over a function with no
   parameter, capture or slot, whose entry block has the label 0."
  (merge {:id 0 :name nil :arity [0 0] :params 0 :captures [] :slots [] :entry 0}
         fields))

(defn block [label instrs term]
  "A block struct of the data form."
  {:label label :instrs instrs :term term})

(defn steps [n]
  "next(p, p + 1) for each point p below N, as [p q]: a body with no branch."
  (->array (map (fn [p] [p (+ p 1)]) (range n))))

(defn exit [p]
  "next(p, [:exit p]): the edge from the Return or the TailCall at P to its exit."
  [p [:exit p]])

(defn function-of [answer i]
  "The struct glean's ANSWER holds for the function at index I of the unit."
  (def fun (get (get answer :functions) i))
  (assert (struct? fun) (string "glean answers function " i))
  fun)

(defn expect-facts [answer i expected]
  "Assert that the function at index I gives exactly the EXPECTED facts. Each
   key's tuples compare as a set, and a key absent from one side has none."
  (def actual (get (function-of answer i) :facts))
  (each k in (distinct (concat (keys actual) (keys expected)))
    (assert (= (set ;(get actual k [])) (set ;(get expected k [])))
            (string "function " i " gives the expected " k " facts"))))
