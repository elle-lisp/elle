(elle/epoch 15)
# audited: 2026-11-17
# The injected abort delivery: one row per route its payload's mint is consumed on, plus the tail-position pair.
#
# docs/impl/region/diagnostics.md
# ── The injected abort delivery ───────────────────────────────────────
# `fiber/abort` installs a payload the CALLER owns, whose one reference answers the
# caller's ARGUMENT release and nothing else. So the injection mints the delivery —
# once, at the seam every route leaves through — and exactly one further release
# consumes it as a RESULT (docs/impl/region/effects.md). Which release that is depends
# on where the injected error stops, and the routes are gauged apart because a mint
# keyed on the route rather than on the injection funds two of them twice.
#
# Nine controls at 0, one per route and per recorded mint:
#
#   `abort-masked`   — the fiber's mask catches, the caller releases the result;
#   `abort-escape`   — the error leaves the fiber, an ancestor `try` absorbs it;
#   `abort-caught`   — a handler INSIDE the body catches, and its own resume
#                      result is the consumer;
#   `abort-own-error`— that body then raises an error of its OWN, which mints its
#                      own delivery, so the abort owes the result nothing;
#   `abort-reraise`  — the body re-raises the injected payload, where value
#                      identity alone cannot tell the two apart;
#   `abort-defer`    — the unwind replays a parked `defer` frame, whose
#                      suspending call runs the result release;
#   `abort-held`     — the fiber is aborted with a value it was already handed,
#                      so its abandoned frame owes that value a release, which
#                      the recorded mint is what stops exempting;
#   `abort-other`    — its pair-control, aborted with a value the fiber does not
#                      hold, isolating the record from the walk;
#   `abort-aborting-frame` — the other side of the record: a literal
#                      materialized straight into the `fiber/abort` argument
#                      lives in the ABORTING frame's slot and nowhere else.
#
# Every one of them discards the abort's result. That is not incidental — see
# the tail-position pair below, which is what the discard is holding constant.
# The soundness complement is `region-fiber-abort-delivery-uaf.lisp`.
(defn ab-mk-caught []
  (fiber/new (fn []
               (protect (yield 1))
               7) |:yield :error|))
(defn ab-mk-masked []
  (fiber/new (fn []
               (yield 1)
               2) |:yield :error|))
(defn ab-hold-then-yield [q]
  (yield q)
  2)
# Each payload names the op's index, so every op builds its own. A payload of
# constants is built once per unit (docs/impl/hoist.md), so a surplus or missing
# release of it moves no gauge. The allocation rows hold each probe to building
# its payload.
(defn abort-rate [subject probe]
  (r:drive subject (r:stmt-run probe) :on [r:objects r:allocs]))
(println "── folded suite: injected abort delivery ──")
(abort-rate "abort-masked"
            (fn [j]
              (let [f (ab-mk-masked)]
                (fiber/resume f)
                (fiber/abort f [j 2 3])
                nil)))
(abort-rate "abort-escape"
            (fn [j]
              (let [p {:error :injected :op j}
                    f (fiber/new (fn []
                                   (yield 1)
                                   2) |:yield|)]
                (fiber/resume f)
                (try
                  (begin
                    (fiber/abort f p)
                    nil)
                  (catch e nil))
                nil)))
(abort-rate "abort-caught"
            (fn [j]
              (let [f (ab-mk-caught)]
                (fiber/resume f)
                (fiber/abort f [j 2 3])
                nil)))
(abort-rate "abort-own-error"
            (fn [j]
              (let [f (fiber/new (fn []
                                   (protect (yield 1))
                                   (error {:own 1})) |:yield :error|)]
                (fiber/resume f)
                (fiber/abort f [j 2 3])
                nil)))
(abort-rate "abort-reraise"
            (fn [j]
              (let [f (fiber/new (fn []
                                   (let [r (protect (yield 1))]
                                     (error (get r 1)))) |:yield :error|)]
                (fiber/resume f)
                (fiber/abort f [j 2 3])
                nil)))
(abort-rate "abort-defer"
            (fn [j]
              (let [f (fiber/new (fn []
                                   (defer
                                     (length [1 2 3 4 5])
                                     (yield 1)
                                     2)) |:yield :error|)]
                (fiber/resume f)
                (fiber/abort f [j 8 9])
                nil)))
(abort-rate "abort-held"
            (fn [j]
              (let [p {:a j}
                    f (fiber/new ab-hold-then-yield |:yield :error|)]
                (fiber/resume f p)
                (fiber/abort f p)
                nil)))
(abort-rate "abort-other"
            (fn [j]
              (let [p {:a j}
                    f (fiber/new ab-hold-then-yield |:yield :error|)]
                (fiber/resume f p)
                (fiber/abort f {:b j})
                nil)))
(abort-rate "abort-aborting-frame"
            (fn [j]
              (let [f (fiber/new (fn []
                                   (yield 1)
                                   2) |:yield|)]
                (fiber/resume f)
                (try
                  (begin
                    (fiber/abort f {:e j})
                    nil)
                  (catch e nil))
                nil)))
# The TAIL-position face of the same abort. The nine controls above discard the
# abort's result, and this one RETURNS it. `abort-tail-discarded` is the
# identical body with a `nil` after the call, so the gap between the pair
# isolates the tail position. `fiber/abort` in tail position is a NATIVE tail
# call that leaves by a signal. But an ABSORBED outcome is the carrier's answer
# rather than an exit, so the frame falls through to the post-`TailCall` block
# and runs the owned-argument releases it holds (docs/impl/region/signalexit.md).
#
# A closed control. The counter-factual reads the answer as an exit, and strands
# three regions: the fiber argument, the closure behind it, and the payload. An
# IMMEDIATE payload removes one of them, and a payload an enclosing binding owns
# removes none. That discriminates it from `abort-mask-caught-literal` below,
# whose whole strand IS the payload, so the two must stay a pair.
#
# It reads 0 on every tier. A compiled frame reaches the same releases through
# its own post-`TailCall` block, so the strand it guards is interpreter
# machinery alone.
(abort-rate "abort-tail-result"
            (fn [j]
              (let [f (ab-mk-caught)]
                (fiber/resume f)
                (fiber/abort f [j 2 3]))))
(abort-rate "abort-tail-discarded"
            (fn [j]
              (let [f (ab-mk-caught)]
                (fiber/resume f)
                (fiber/abort f [j 2 3])
                nil)))
# The payload's OWN region, where one frame both allocates it and consumes the
# abort's result. The frame owes TWO releases on that one region — the argument
# and the result — and holds two references to fund them: its allocation's, and
# the injection's delivery mint. The post-`TailCall` block carries both, which is
# what a skipped block costs one region per abort.
#
# This probe needs three ingredients. The fiber's MASK catches, so the caller
# receives the payload back. The payload is a literal materialized in the
# aborting frame. And that frame consumes the result. `abort-mask-caught-bound`
# is the pair-control that removes the second: the same abort over a payload an
# enclosing binding owns, whose own release then covers it. A closed control.
#
# It is NOT `abort-tail-result` seen smaller. The two must stay a pair, because
# otherwise only resemblance tells them apart: both need the result in tail
# position, and both flatten when it is bound. An IMMEDIATE payload separates
# them. It has no region at all, so this shape reads 0 there while
# `abort-tail-result` still counts the fiber. `abort-discard` above sits one mask
# bit away: with `|:yield|` the injected error escapes the fiber instead of being
# caught by the mask, and that route reclaims.
(defn ab-mk-mask-caught []
  (fiber/new (fn []
               (yield 1)
               9) |:yield :error|))
(abort-rate "abort-mask-caught-literal"
            (fn [j]
              (let [f (ab-mk-mask-caught)]
                (fiber/resume f)
                (protect (fiber/abort f [j])))))
(abort-rate "abort-mask-caught-bound"
            (fn [j]
              (let [p [j]
                    f (ab-mk-mask-caught)]
                (fiber/resume f)
                (protect (fiber/abort f p)))))
# `fiber/refuse` shares the injection seam with `fiber/abort`
# (`inject_error_at_suspension`) and leaves by the same `SIG_ABORT`. So it
# reaches the absorbed-carrier fall-through by the same route, and needs its own
# reading. Nothing about the seam distinguishes the two, so a change that
# reintroduces the strand for one reintroduces it for both. The pair is the same
# as the abort's: the result RETURNED, and the identical body with a `nil` after
# the call.
(abort-rate "refuse-tail-result"
            (fn [j]
              (let [f (ab-mk-caught)]
                (fiber/resume f)
                (fiber/refuse f [j 2 3]))))
(abort-rate "refuse-tail-discarded"
            (fn [j]
              (let [f (ab-mk-caught)]
                (fiber/resume f)
                (fiber/refuse f [j 2 3])
                nil)))
