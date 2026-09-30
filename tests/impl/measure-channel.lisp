(elle/epoch 13)
# audited: 2026-09-30
# measure-channel.lisp — one probe read on two gauges through the ratchet, so
# the reading line has a producer small enough to drive from a test.
#
# docs/test-store.md
#
# The leak dashboards are the real producers, and each costs a minute. This
# file drives the same instrument over one shape that keeps every object it
# makes, so a test can assert what a reading becomes in the store without
# paying for a dashboard. The shape is the live-growth discriminator's own,
# so its rows are growth floors: a run in which it reads flat is a dead gauge,
# and the one thing this file must never do is pass on a flat gauge.
(def r ((import "std/ratchet")))

(def @sink @[])
(r:rate "channel-keep" (fn [j] (push sink {:k j})) :on [r:objects r:regions]
        :block 100 :min 4 :max 30)
(r:report)
(println "measure-channel: ok")
