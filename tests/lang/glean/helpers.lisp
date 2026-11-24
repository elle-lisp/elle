(elle/epoch 18)
# audited: 2026-11-24
# What glean's test files share: builders for a unit in the data form, and the checks of a function's answer.
# tools/glean/overview.md
# tools/glean/present.md
# tests/lang/overview.md

(def {:heard heard} ((import "../../../tools/glean/noise")))

(defn quiet [f]
  "F, with each :glean/noise signal it raises heard and dropped."
  (fn [& args] (first (heard (fn [] (f ;args))))))

(defn function [fields]
  "A function struct of the data form: FIELDS over a function with no
   parameter, capture or slot, whose entry block has the label 0."
  (merge {:id 0 :name nil :arity [0 0] :params 0 :captures [] :slots [] :entry 0}
         fields))

# Not `block`: that is a special form, and a call of a function bound to the
# name runs the form instead, which answers its last expression.
(defn basic-block [label instrs term]
  "A block struct of the data form."
  {:label label :instrs instrs :term term})

(defn invoke [kind dst func args]
  "A call instruction of KIND, as the lowerer writes a call it checked."
  {:kind kind
   :dst dst
   :func func
   :args args
   :arity-checked true
   :region 0
   :moves []})

(defn fresh-array [dst region]
  "A MakeArrayMut of no element into the static region REGION."
  {:kind :make-array-mut :dst dst :elements [] :region region})

(defn load-slot [dst slot]
  "A LoadLocal of SLOT into the register DST."
  {:kind :load-local :dst dst :slot slot})

(defn store-slot [slot src]
  "A StoreLocal of the register SRC into SLOT."
  {:kind :store-local :slot slot :src src})

(defn release-value [src]
  "A DecrefValueRegion of the register SRC."
  {:kind :decref-value-region :src src})

(defn nil-const [dst]
  "A Const of nil into the register DST."
  {:kind :const :dst dst :value nil})

(defn return-of [src]
  "A Return of the register SRC."
  {:kind :return :value src})

(defn jump-to [label]
  "A Jump to the block LABEL."
  {:kind :jump :target label})

(defn branch-on [c then-label else-label]
  "A Branch on the register C."
  {:kind :branch :cond c :then-label then-label :else-label else-label})

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

(defn expect-sets [actual ks expected what]
  "Assert that ACTUAL holds under each key of KS exactly what EXPECTED holds
   there. Each key's elements compare as a set, and a key absent from a side
   has none. WHAT names ACTUAL in the message."
  (each k in ks
    (assert (= (set ;(get actual k [])) (set ;(get expected k [])))
            (string what " answers the expected " k))))

(defn expect-facts [answer i expected]
  "Assert that the function at index I gives exactly the EXPECTED facts."
  (def actual (get (function-of answer i) :facts))
  (expect-sets actual (distinct (concat (keys actual) (keys expected))) expected
               (string "function " i)))

(defn expect-present [answer i expected]
  "Assert that the function at index I of present's ANSWER gives exactly the
   EXPECTED releases, hand-overs, holds and overs."
  (expect-sets (function-of answer i) [:released :handed :held :over] expected
               (string "function " i)))

(defn expect-holds [answer i expected]
  "Assert that the function at index I of present's ANSWER counts at birth,
   holds and has ended exactly what EXPECTED gives."
  (expect-sets (function-of answer i) [:counted :holding :ended] expected
               (string "function " i)))

(defn expect-diff [answer i expected]
  "Assert that the function at index I of the diff's ANSWER reads exactly the
   EXPECTED early, late and none values."
  (expect-sets (function-of answer i) [:early :late :none] expected
               (string "function " i)))

(defn span [v from to]
  "[V p] for each point p from FROM to TO."
  (->array (map (fn [p] [v p]) (range from (+ to 1)))))

(defn refused [thunk reason]
  "Assert that THUNK refuses with a :present-error whose reason is REASON."
  (def [ok? err] (protect (thunk)))
  (assert (not ok?) (string "present refuses with " reason))
  (assert (= (get err :error) :present-error) "the refusal is a :present-error")
  (assert (= (get err :reason) reason) (string "the refusal names " reason)))
