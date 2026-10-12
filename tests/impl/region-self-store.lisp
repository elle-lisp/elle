(elle/epoch 14)
# audited: 2026-10-11
# A mutable container that holds itself frees at its last release.
# docs/impl/region/rules.md
#
# Each shape builds a container, stores the container into itself through one
# store funnel, and drops it. Two hundred calls must leave the live region
# count where it was.
#
# The counter-factual: a store funnel that counts a reference from a region to
# itself raises a count that only the free cascade could lower, and the cascade
# runs only once the count is zero. So every container outlives its last holder,
# and the count grows by one per call.
#
# The discriminator keeps each container in a program-lifetime array, so the
# count grows by one per call. That proves the gauge moves.

(defn growth [thunk]
  "Live regions gained across 200 calls of THUNK, after one warm-up call."
  (thunk)
  (let [before (arena/region-count)]
    (repeat 200 (thunk))
    (- (arena/region-count) before)))

(def @kept @[])

(assert (> (growth (fn []
                     (push kept @[])
                     nil)) 150)
        "the gauge moves: 200 kept arrays grow the live region count")

(assert (<= (growth (fn []
                      (let [a @[]]
                        (push a a)
                        nil))) 2)
        "an @array pushed into itself frees at its last release")

(assert (<= (growth (fn []
                      (let [a @[1]]
                        (insert a 0 a)
                        nil))) 2)
        "an @array inserted into itself frees at its last release")

(assert (<= (growth (fn []
                      (let [a @[1]]
                        (put a 0 a)
                        nil))) 2)
        "an @array stored over its own element frees at its last release")

(assert (<= (growth (fn []
                      (let [s @{}]
                        (put s :me s)
                        nil))) 2)
        "an @struct put into itself frees at its last release")

(assert (<= (growth (fn []
                      (let [b (box nil)]
                        (rebox b b)
                        nil))) 2)
        "a box stored into itself frees at its last release")

(println "region-self-store: OK")
