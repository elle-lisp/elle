(elle/epoch 18)
# audited: 2026-11-24
# The values whose region a state of present's walk holds, and those whose region has ended there.
# tools/glean/present.md

(fn []
  (def {:emit emit :record record} ((import "./regions")))

  (defn held? [state tok]
    "Whether the function holds the region TOK: it counts it above zero, the
  activation owns it, or a region the function holds owns it."
    (def rec (record state tok))
    (def mode (get rec :mode))
    (cond
      (= mode :counted) (> (get rec :count) 0)
      (= mode :activation) true
      (= mode :gone) false
      (held? state (get mode 1))))

  (defn note [ctx state p]
    "Answer [v P] under :holding for each value whose region STATE holds, and
  under :ended for each value whose region has ended. The names the activation
  keeps for its exits are held."
    (each [tok rec] in (pairs (get state :regions))
      (def names (union (get rec :values) (get rec :carries)))
      (cond
        (= (get rec :mode) :gone) (emit ctx :ended names p)
        (held? state tok) (emit ctx :holding names p)))
    (emit ctx :holding (get state :dues) p))

  {:note note})
