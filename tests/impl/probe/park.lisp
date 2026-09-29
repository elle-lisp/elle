(elle/epoch 12)
# audited: 2026-09-08
# The activation-adopt bodies a park crosses, and the three controls that remove one ingredient each.
#
# docs/impl/region/diagnostics.md
# The activation-adopt scope crossed by a park — the park split's end-to-end
# family. `ap-adopting-body` is the capture-back-edge shape (`capture-backedge`
# below) with a `(yield j)` INSIDE the scope enclosing the members'
# allocations. The walk keys a second adopt after the last member allocation
# and ahead of the yield (docs/impl/region/owner.md § "Owner nodes" — "The park
# split"), so the parked frame carries the members in its activation owner
# node and every abandonment route frees them: the handle drop's free-path
# discharge, the abort's discharge, and the cancel's terminal teardown alike.
# The set must stay together, because only its gaps attribute a regression:
# `ap-before-body` differs ONLY in the yield's position (after the scope
# closes, so the single scope-exit adopt covers it), `ap-plain-body` removes
# the SCC, `ap-nopark-body` removes the park, and `adopt-complete` removes the
# abandonment. The completion control also settles admission: the Shared
# baseline leaks this cycle per call, so completion at 0 proves the activation
# cut is active for the shape — the park did not refuse it.
(defn ap-adopting-body [j]
  (let [root @[]
        m @[]]
    (let [c (fn [] (length m))]
      (push m c)
      (c)
      (push root m)
      (yield j)
      (length root))))
(defn ap-plain-body [j]
  (let [root @[]
        m @[]]
    (push root m)
    (yield j)
    (length root)))
(defn ap-nopark-body [j]
  (let [root @[]
        m @[]]
    (let [c (fn [] (length m))]
      (push m c)
      (c)
      (push root m)
      (length root))))
(defn ap-before-body [j]
  (begin
    (let [root @[]
          m @[]]
      (let [c (fn [] (length m))]
        (push m c)
        (c)
        (push root m)))
    (yield j)
    j))
# Direct-loop class. Each entry: [label (fn [j] body) rate].
# j varies the input (faithful to the originals' loop variable i). Pins are the
# TRUE CURRENT rate the estimator measures — cross-validated against the source
# files' own slope, several of which are stale (the files are RED there).
