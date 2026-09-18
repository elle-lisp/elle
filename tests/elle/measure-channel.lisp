(elle/epoch 12)
# audited: 2026-09-17
# measure-channel.lisp — one probe read on two gauges, so the measurement
# channel has a dashboard small enough to drive from a test.
#
# docs/test-store.md
#
# The leak dashboards (oracle.lisp, plumb.lisp) are the channel's real callers,
# and each costs a minute of adaptive probing. This file drives the same
# instrument — the shared estimator, `measure-2`, `show` — over one probe with
# a few blocks, so a test can assert what a verdict becomes in the store
# without paying for a dashboard.
(include-file "lib/estimator.lisp")

# A probe that keeps every object it makes: the module-level sink holds each
# struct forever, so the object count must climb ~1/op. This is the gauge-live
# discriminator shape (oracle.lisp § "The gauge-live discriminator") — a dead
# gauge reads ~0 and would paint every verdict green, so the one assertion here
# is that the gauge moved.
(def @sink @[])
(defn probe-keep [j]
  (push sink {:k j}))

# Both readings are declared by-design BEFORE `show` classifies them: growth is
# what this probe is for, so it displays (and records) :growth rather than
# counting as a defect.
(put by-design "channel-keep" true)
(put by-design "channel-keep@regions" true)

(println "── measure-channel ──")
(def readings
  (measure-2 "channel-keep" (fn [b] (run-thunk-block probe-keep b)) count-gauge
             0.4 0.5 "regions" region-gauge 0.4 0.5 100 4 30))
(show (get readings 0))
(show (get readings 1))
(assert (= (get (get readings 0) :verdict) :open)
        (string "GAUGE DEAD: a probe that keeps every object it makes read "
                (get (get readings 0) :verdict)
                " on the object count — no verdict in this run means anything"))
(println "measure-channel: ok")
