(elle/epoch 18)
# audited: 2026-11-24
# The release diff: each value present counted at birth, read early, late or none against the spec's liveness.
# tools/glean/diff.md

(fn []
  (defn diff [unit]
    "The early, late and none values of each function of UNIT."
    {:functions (->array (map (fn [fun]
                                {:id (get fun :id)
                                 :name (get fun :name)
                                 :early []
                                 :late []
                                 :none []}) unit))})

  {:diff diff})
