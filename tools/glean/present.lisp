(elle/epoch 18)
# audited: 2026-11-24
# The present releases: where each function's count instructions end its hold on each value's region.
# tools/glean/present.md

(fn []
  (defn present [unit]
    "Each function of UNIT, with no release read."
    {:functions (->array (map (fn [fun]
                                {:id (get fun :id) :name (get fun :name)}) unit))})

  {:present present})
