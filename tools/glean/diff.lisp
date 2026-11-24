(elle/epoch 18)
# audited: 2026-11-24
# The release diff: each value present counted at birth, read early, late or none against the spec's liveness.
# tools/glean/diff.md

(fn []
  (def proof ((import "std/proof")))
  (def kinds ((import "./kinds")))
  (def {:view view} ((import "./points")))
  (def {:glean glean} ((import "./glean")))
  (def {:heard heard} ((import "./noise")))
  (def {:present present} ((import "./present")))
  (def {:returning returning} ((import "../../docs/spec/return.md")))
  (def {:with-captures with-captures :live-all live-all}
    ((import "../../docs/spec/capture.md")))
  (def {:live-as-of live-as-of} ((import "../../docs/spec/liveness.md")))

  # The kinds that only move a value.
  (def moves
    |:load-local :store-local :load-capture :load-capture-raw :const
     :value-const|)

  (defn judged [fun]
    "The points of FUN's steps the diff judges, as a set: neither a count
  instruction nor a move."
    (def nodes (get (view fun) :nodes))
    (set ;(filter (fn [p]
                    (def kind (get (get (get nodes p) :node) :kind))
                    (not (or (has? kinds:counts kind) (has? moves kind))))
                  (range (length nodes)))))

  (defn live-of [facts]
    "live-as(v, p) as a set of [v p], from glean's FACTS once each returned
  value is live at its exit and each late capture is a store."
    (def spec (with-captures (returning facts)))
    (set ;(proof:elements (live-as-of spec (live-all spec)))))

  (defn function-diff [fun gleaned held]
    "The struct the diff answers for FUN, from glean's struct GLEANED and
  present's struct HELD."
    (def counted (set ;(get held :counted)))
    (def steps (judged fun))
    (def live (live-of (get gleaned :facts)))
    (defn read [key keep?]
      (->array (filter (fn [[v p]]
                         (and (contains? counted v) (contains? steps p)
                              (keep? (contains? live [v p])))) (get held key))))
    {:id (get fun :id)
     :name (get fun :name)
     :early (read :ended (fn [live?] live?))
     :late (read :holding not)
     :none (->array (filter (fn [[v p]] (contains? counted v)) (get held :held)))})

  (defn diff [unit]
    "The early, late and none values of each function of UNIT."
    (def [gleaned _] (heard (fn [] (glean unit))))
    (def held (present unit))
    {:functions (->array (map (fn [i]
                                (function-diff (get unit i)
                                  (get (get gleaned :functions) i)
                                  (get (get held :functions) i)))
                              (range (length unit))))})

  {:diff diff})
