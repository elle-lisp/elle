(elle/epoch 12)
# audited: 2026-09-29
# An aborted child's replayed frame consumes the reference the abort's injection minted, and reads no freed region.
# docs/impl/region/park.md
#
# The sidecar arms guardfree and keeps the JIT off, so a stale-region deref
# faults at the replay.
#
# WHAT IT PINS — the abort-delivery retain (docs/impl/region/park.md, the
# delivery rule): a replayed frame's pending
# release consumes one owning reference of the value it is resumed with. A
# normally-completing child funds it with its Return's ReturnValue retain;
# an ABORTED child's error exit runs no Return, so the reference the replay
# consumes is the one `fiber/abort`'s injection minted — this replay is one
# of the four consumers that single mint answers for
# (docs/impl/region/effects.md).
#
# The shape: a spawned fiber parks inside `(protect (ev/sleep …))` — an
# io-parked protect child under a FiberResume frame — and the scheduler
# aborts it with a FRESH HEAP struct payload (`ev/abort` → `handle-abort` →
# `(fiber/abort target {:error :aborted})`). The abort unwinds the protect
# child to :error and replays the wrapper frames with the struct as the
# resume value; the wrapper's pending release, the protect tuple's death,
# and the child fiber's teardown then consume every granted reference —
# without the delivery retain, the scheduler frame's own return of the
# abort result is a borrow into a freed region (stale once ids recycle).
# tests/lang/grpc.lisp's `with-server` teardown is the full-network
# witness of the same shape.

(defn one-abort []
  (ev/run (fn []
            (let [sf (ev/spawn (fn []
                                 (let [[ok? _] (protect (ev/sleep 5))]
                                   nil)))]
              (ev/sleep 0.01)
              (protect (ev/abort sf))
              nil))))

# The minimal detonator (a missing retain faults on the first pass), then churn
# so a regression that merely under-counts (masked by a longer-lived holder)
# still detonates on a recycled id.
(one-abort)
(def @i 0)
(while (< i 30)
  (one-abort)
  (assign i (+ i 1)))

(println "region-fiber-abort-io-protect: ok")
