(elle/epoch 18)
# audited: 2026-11-21
# Glean: the base facts the Datalog rules read, from each function of a unit in the data form.
# tools/glean/overview.md

(fn []
  (defn glean [unit]
    "The facts and marks of each function of UNIT, and the closure constants it meets."
    {:functions [] :closures []})

  {:glean glean})
