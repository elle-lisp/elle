(elle/epoch 15)
# audited: 2026-10-05
# ── The boundary discard chokepoint, exercised in a loop ──
#
# A squelch/attune signal-violation reaches `VM::discard_suspended_frames`
# (src/vm/core/discard.rs) through `VM::boundary_violation`: the frames parked
# between the signal site and the boundary are abandoned, and the chokepoint
# runs what each frame still owed and releases nothing else
# (docs/impl/region/diagnostics.md). The frames' `activation_region_map`
# regions may be shared with an outer, non-discarded frame or with the
# activation that catches the violation, so a discard that over-releases them
# frees live state: the catch handler's next read, or a later scheduler pump,
# touches a freed page. Looping the abort makes an over-release deterministic,
# because the shared regions' counts drain within a few iterations, and a
# debug build's generation check panics on a stale read of a freed region.
#
# This file pins that the discard frees nothing a live frame still counts on.
# That the discard does free what the parked frames owed is pinned Rust-side,
# in `runtime::tests::ownership::fnode::discard`.

# ── S1: an IO-blocked attune, looped ──
# `println`'s SIG_IO violates the |:error| allow mask, so every call aborts
# through the discard with the io park chain in flight. The violation must
# stay readable and the abort must not perturb later iterations.
(def io-fn
  (fn []
    (println "side effect")
    :done))
(def blocked (attune |:error| io-fn))
(var i 0)
(while (< i 20)
  (let [[ok? err] (protect (blocked))]
    (assert (not ok?) "io-blocked attune: the violation is caught")
    (assert (= (get err :error) :signal-violation)
            "io-blocked attune: the discard leaves a readable signal-violation"))
  (assign i (+ i 1)))
(println "squelch-discard-io-loop: ok")

# ── S2: a squelched yield through a nested call, looped ──
# The yield parks a chain through `wrapper` before squelch converts it at
# the boundary; the discard drops that chain (the handle_emit park, not the
# io park — the other frame shape the chokepoint sees).
(def yielder (fn [] (yield 7)))
(def wrapper (fn [] (yielder)))
(def squelched (squelch :yield wrapper))
(var j 0)
(while (< j 20)
  (let [[ok? err] (protect (squelched))]
    (assert (not ok?) "squelched yield: the violation is caught")
    (assert (= (get err :error) :signal-violation)
            "squelched yield: the discard leaves a readable signal-violation"))
  (assign j (+ j 1)))
(println "squelch-discard-yield-loop: ok")

# ── S3: the scheduler survives the discards ──
# Fresh fiber machinery after 40 aborts: a yield round-trip and a completion
# must read intact state (an over-releasing discard drains the scheduler's
# own regions, and this is where the stale read would surface).
(def coro (fiber/new (fn [] (yield 1)) |:yield|))
(fiber/resume coro nil)
(assert (= (fiber/value coro) 1) "post-discard: a fresh fiber yields intact")

(println "region-squelch-discard: ok")
