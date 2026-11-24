(elle/epoch 18)
# audited: 2026-11-24
# Witnesses: a fence's function run in a child, to confirm a leak, a use after free or a hold that grows.
# tools/glean/witness.md

(fn []
  (defn witness [elle entry readings]
    "The verdict on each of READINGS for the function ENTRY names."
    {})

  {:witness witness})
