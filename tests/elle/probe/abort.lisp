(elle/epoch 12)
# audited: 2026-09-08
# The injected abort delivery: one row per route its payload's mint is consumed on, plus the tail-position pair.
#
# docs/impl/region/diagnostics.md
# ── The injected abort delivery ───────────────────────────────────────
# `fiber/abort` installs a payload the CALLER owns, whose one reference answers
# the caller's ARGUMENT release and nothing else. So the injection mints the
# delivery — once, at the seam every route leaves through — and exactly one
# further release consumes it as a RESULT (docs/impl/region/effects.md
# § `Delivers`). Which release that is depends on where the injected error
# stops, and the routes are gauged apart because a mint keyed on the route
# rather than on the injection funds two of them twice.
#
# Nine CLOSED controls (undeclared, like `rest-array-copy`), one per route and
# per recorded mint:
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
(println "── folded suite: injected abort delivery ──")
(pin (measure-core "abort-masked"
                   (stmt-run (fn []
                               (let [f (ab-mk-masked)]
                                 (fiber/resume f)
                                 (fiber/abort f [1 2 3])
                                 nil))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "abort-escape"
                   (stmt-run (fn []
                               (let [p {:error :injected}
                                     f (fiber/new (fn []
                                       (yield 1)
                                       2) |:yield|)]
                                 (fiber/resume f)
                                 (try
                                   (begin
                                     (fiber/abort f p)
                                     nil)
                                   (catch e nil))
                                 nil))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "abort-caught"
                   (stmt-run (fn []
                               (let [f (ab-mk-caught)]
                                 (fiber/resume f)
                                 (fiber/abort f [1 2 3])
                                 nil))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "abort-own-error"
                   (stmt-run (fn []
                               (let [f (fiber/new (fn []
                                       (protect (yield 1))
                                       (error {:own 1})) |:yield :error|)]
                                 (fiber/resume f)
                                 (fiber/abort f [1 2 3])
                                 nil))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "abort-reraise"
                   (stmt-run (fn []
                               (let [f (fiber/new (fn []
                                       (let [r (protect (yield 1))]
                                         (error (get r 1)))) |:yield :error|)]
                                 (fiber/resume f)
                                 (fiber/abort f [1 2 3])
                                 nil))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "abort-defer"
                   (stmt-run (fn []
                               (let [f (fiber/new (fn []
                                       (defer
                                         (length [1 2 3 4 5])
                                         (yield 1)
                                         2)) |:yield :error|)]
                                 (fiber/resume f)
                                 (fiber/abort f [7 8 9])
                                 nil))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "abort-held"
                   (stmt-run (fn []
                               (let [p {:a 1}
                                     f (fiber/new ab-hold-then-yield
                                     |:yield :error|)]
                                 (fiber/resume f p)
                                 (fiber/abort f p)
                                 nil))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "abort-other"
                   (stmt-run (fn []
                               (let [p {:a 1}
                                     f (fiber/new ab-hold-then-yield
                                     |:yield :error|)]
                                 (fiber/resume f p)
                                 (fiber/abort f {:b 2})
                                 nil))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "abort-aborting-frame"
                   (stmt-run (fn []
                               (let [f (fiber/new (fn []
                                       (yield 1)
                                       2) |:yield|)]
                                 (fiber/resume f)
                                 (try
                                   (begin
                                     (fiber/abort f {:e 1})
                                     nil)
                                   (catch e nil))
                                 nil))) count-gauge 100 6 60 0.4 0.5) 0)
# The TAIL-position face of the same abort. The nine controls above all discard
# the abort's result; this one RETURNS it, which is the whole difference —
# `abort-tail-discarded` is the identical body with a `nil` after the call, so
# the gap between the pair isolates the tail position rather than the abort.
# `fiber/abort` in tail position is a NATIVE tail call that leaves by a signal,
# but an ABSORBED outcome is the carrier's answer rather than an exit: the frame
# is still there, so it falls through to the post-`TailCall` block and runs the
# owned-argument releases that block holds (docs/impl/region/mechanism.md § "A
# carrier that comes back with a result never left the frame"). A closed control
# now. The counter-factual — reading the answer as an exit — strands the fiber
# argument, the closure behind it, and the payload: three regions, of which an
# IMMEDIATE payload removes one and a payload an enclosing binding owns removes
# none. That is the discriminator against `abort-mask-caught-literal` below,
# whose whole strand IS the payload, and it is why the two must stay a pair.
#
# It reads 0 on every tier, and did so under `--jit=eager` before the fall-through
# landed: a compiled frame reaches the same releases through its own
# post-`TailCall` block, so the strand was interpreter machinery alone.
(pin (measure-core "abort-tail-result"
                   (stmt-run (fn []
                               (let [f (ab-mk-caught)]
                                 (fiber/resume f)
                                 (fiber/abort f [1 2 3])))) count-gauge 100 6 60
                   0.4 0.5) 0)
(pin (measure-core "abort-tail-discarded"
                   (stmt-run (fn []
                               (let [f (ab-mk-caught)]
                                 (fiber/resume f)
                                 (fiber/abort f [1 2 3])
                                 nil))) count-gauge 100 6 60 0.4 0.5) 0)
# The payload's OWN region, where one frame both allocates it and consumes the
# abort's result. The frame owes TWO releases on that one region — the argument
# and the result — and holds two references to fund them: its allocation's, and
# the injection's delivery mint. The post-`TailCall` block carries both, which is
# what a skipped block costs one region per abort. This probe needs all three
# ingredients — the fiber's MASK catches (so the caller receives the payload
# back), the payload is a literal materialized in the aborting frame, and that
# frame consumes the result — and `abort-mask-caught-bound` is the pair-control
# that removes the second, the same abort over a payload an enclosing binding
# owns, whose own release then covers it. A closed control now.
#
# It is NOT `abort-tail-result` seen smaller, and the two must stay a pair because
# resemblance is all there is to go on otherwise: both need the result in tail
# position and both flatten when it is bound. An IMMEDIATE payload separates them
# — it has no region at all, so this shape reads 0 there while
# `abort-tail-result` still counts the fiber. `abort-discard` above sits one mask
# bit away: with `|:yield|` the injected error escapes the fiber instead of being
# caught by the mask, and that route reclaims.
(defn ab-mk-mask-caught []
  (fiber/new (fn []
               (yield 1)
               9) |:yield :error|))
(pin (measure-core "abort-mask-caught-literal"
                   (stmt-run (fn []
                               (let [f (ab-mk-mask-caught)]
                                 (fiber/resume f)
                                 (protect (fiber/abort f "boom"))))) count-gauge
                   100 6 60 0.4 0.5) 0)
(pin (measure-core "abort-mask-caught-bound"
                   (stmt-run (fn []
                               (let [p "boom"
                                     f (ab-mk-mask-caught)]
                                 (fiber/resume f)
                                 (protect (fiber/abort f p))))) count-gauge 100
                   6 60 0.4 0.5) 0)
# `fiber/refuse` shares the injection seam with `fiber/abort`
# (`inject_error_at_suspension`) and leaves by the same `SIG_ABORT`, so it reaches
# the absorbed-carrier fall-through by the same route and needs its own reading:
# nothing about the seam distinguishes the two, so a change that reintroduces the
# strand for one reintroduces it for both. The pair is the same as the abort's —
# the result RETURNED, and the identical body with a `nil` after the call.
(pin (measure-core "refuse-tail-result"
                   (stmt-run (fn []
                               (let [f (ab-mk-caught)]
                                 (fiber/resume f)
                                 (fiber/refuse f [1 2 3])))) count-gauge 100 6
                   60 0.4 0.5) 0)
(pin (measure-core "refuse-tail-discarded"
                   (stmt-run (fn []
                               (let [f (ab-mk-caught)]
                                 (fiber/resume f)
                                 (fiber/refuse f [1 2 3])
                                 nil))) count-gauge 100 6 60 0.4 0.5) 0)
