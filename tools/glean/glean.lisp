(elle/epoch 18)
# audited: 2026-11-22
# Glean: the base facts the Datalog rules read, from each function of a unit in the data form.
# tools/glean/overview.md

(fn []
  (def {:function-facts function-facts} ((import "./rules")))

  (defn glean [unit]
    "The facts and marks of each function of UNIT, and the closure constants it meets."
    (def ctx {:natives (compile/primitives) :closures @[]})
    (def functions (->array (map (fn [fun] (function-facts ctx fun)) unit)))
    {:functions functions :closures (freeze (get ctx :closures))})

  {:glean glean})
