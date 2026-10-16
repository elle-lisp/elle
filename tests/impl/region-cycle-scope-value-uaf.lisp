(elle/epoch 14)
# audited: 2026-10-06
# A cycle whose member is its scope's value stays live for the consumer that reads it after the scope.
# docs/impl/region/ownership.md
#
# The sidecar arms guardfree.
#
# `a` and `b` hold each other, and the `let` that builds them answers `a`. A
# rootless cycle frees as one group at the innermost scope enclosing its
# allocations, which is that `let`, and the `let` ends before the consumer
# reads `a`. A group freed there frees `a` under the read, which guardfree
# turns into a fault. The cycle stays on per-region RC instead.

(def at-top-level
  (length (let [a @[]
                b @[]]
            (%array-push a b)
            (%array-push b a)
            a)))
(assert (= at-top-level 1) "the scope's value holds its one member")

(defn in-a-function []
  (length (let [a @[]
                b @[]]
            (%array-push a b)
            (%array-push b a)
            a)))
(assert (= (in-a-function) 1) "and the same inside a function body")

# Repeated, so that a page freed under a stale read is recycled between rounds.
(def @n 0)
(while (%lt n 50)
  (assert (= (in-a-function) 1) "every round reads the live member")
  (assign n (%add n 1)))
