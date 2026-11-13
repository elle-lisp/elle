(elle/epoch 15)
# audited: 2026-11-12
# A native tail call that leaves by a signal consumes the borrowed-argument retain its fall-through would have.
# docs/impl/region/signalexit.md
#
# A tail call hands its callee one fresh owning reference per BORROWED argument.
# The closure env owns a captured upvalue, not this activation, so pure-moving
# it would hand the callee a reference the caller never had
# (docs/impl/region/rules.md Rule 5). That retain has exactly one consumer per
# path: a frame-replacing CLOSURE callee's owned-param release, or — a native
# pushing no bytecode frame — the post-`TailCall` fall-through block's own
# release.
#
# A native that leaves by a SIGNAL reaches neither. The fall-through block
# belongs to one outcome, normal completion. An error, a fiber carrier
# (`fiber/resume`/`fiber/abort`/`fiber/propagate`) and a capability denial each
# abandon this frame's continuation. So the retain strands once per call, and
# everything the argument's free cascade would have reclaimed strands behind
# it. For a fiber carrier that is the fiber itself, hence its body closure, its
# captures and its parked payload.
#
# The signal exit therefore consumes the retain itself, and stamps the stash
# local `nil` as it does. A frame that leaves by a signal can still be REPLAYED,
# because an `:error` fiber is resumable for the restarts system. The replayed
# `DecrefValueRegion` must then find nothing left to release.
#
# A SUSPEND consumes nothing. It parks the continuation at the post-`TailCall`
# ip, and its payload may embed the argument uncounted, so the retain stays for
# the block past the call. A resume replays that block, and the frame's release
# table hands the release to a park nobody resumes (rows f-h).
#
# This file is the LEAK gauge — an `arena/count` delta over a fixed window,
# BOUNDED for every subject. The soundness complement is
# region-tail-signal-exit-uaf.lisp; the per-op rate is the oracle's
# `abort-discard` row (tests/impl/probe/concurrent.lisp).

(def window 500)

(defn measure [thunk warm window]
  (var i 0)
  (while (%lt i warm)
    (thunk)
    (assign i (%add i 1)))
  (def before (arena/count))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  (%sub (arena/count) before))

# subjects ─────────────────────────────────────────────────────────────────────

# (a) the FIBER-CARRIER exit. `fiber/abort` returns its fiber ARGUMENT as the
# signal payload, so the handler replaces it before any caller release runs and
# the frame leaves through the signal. The stranded retain is the fiber's own,
# so the body closure and everything the parked frame holds strand with it.
(defn abort-carrier []
  (let [f (fiber/new (fn []
                       (yield 1)
                       9) |:yield|)]
    (fiber/resume f)
    (try
      (fiber/abort f 7)
      (catch e nil))
    nil))

# (b) the same abort with a HEAP payload, so the exit's release runs beside a
# payload the signal machinery installs — the two must not be confused for each
# other.
(defn abort-heap-payload []
  (let [f (fiber/new (fn []
                       (yield 1)
                       9) |:yield|)]
    (fiber/resume f)
    (try
      (fiber/abort f (string "boom"))
      (catch e nil))
    nil))

# (c) a RESTARTED `:error` fiber: the abort parks the frame, the resume replays
# the post-`TailCall` block, and the release must still have run exactly once.
(defn abort-then-restart []
  (let [f (fiber/new (fn []
                       (yield 1)
                       9) |:yield :error|)]
    (fiber/resume f)
    (try
      (fiber/abort f 7)
      (catch e nil))
    (try
      (fiber/resume f)
      (catch e nil))
    nil))

# controls ─────────────────────────────────────────────────────────────────────

# (d) a borrowed argument whose native COMPLETES: the fall-through runs and
# consumes the retain. It is bounded with or without the signal exit's release.
(defn ok-borrow []
  (let [f (fiber/new (fn []
                       (yield 1)
                       9) |:yield|)]
    (fiber/resume f)
    (fiber/status f)
    nil))

# (e) an error raised with no borrowed argument at all — no retain to consume,
# so the exit must add no release of its own.
(defn err-plain []
  (try
    (string/trim 7)
    (catch e nil))
  nil)

# the suspend ──────────────────────────────────────────────────────────────────

# (f)-(h) a SUSPENDING io op in tail position on a destructured leaf. The
# relocation releases the leaf's container ahead of the call, so the call's
# retain is the leaf's last reference, and the exit leaves it standing for the
# release past the `TailCall`. A resume replays that release. A park an abort or
# a drop ends reaches it only through the frame's release table, and a bare
# release that no table names strands the leaf once per park. The mask brings
# each park back to this driver, so the request never reaches a backend.
(defn park-on-leaf []
  (let [f (fiber/new (fn []
                       (let [[label data] [(string "label") (string "payload")]]
                         (port/write (port/stdout) data))) |:io|)]
    (fiber/resume f)
    f))

(defn leaf-park-resume []
  (fiber/resume (park-on-leaf) 7)
  nil)

(defn leaf-park-abort []
  (let [f (park-on-leaf)]
    (try
      (fiber/abort f 7)
      (catch e nil))
    nil))

(defn leaf-park-drop []
  (park-on-leaf)
  nil)

# measurement ──────────────────────────────────────────────────────────────────

(def d-abort (measure abort-carrier 20 window))
(def d-heap (measure abort-heap-payload 20 window))
(def d-restart (measure abort-then-restart 20 window))
(def d-leaf-resume (measure leaf-park-resume 20 window))
(def d-leaf-abort (measure leaf-park-abort 20 window))
(def d-leaf-drop (measure leaf-park-drop 20 window))
(def d-ok (measure ok-borrow 20 window))
(def d-plain (measure err-plain 20 window))

(println "region-tail-signal-exit over " window " iters (object deltas):")
(println "  abort-carrier      " d-abort)
(println "  abort-heap-payload " d-heap)
(println "  abort-then-restart " d-restart)
(println "  leaf-park-resume   " d-leaf-resume)
(println "  leaf-park-abort    " d-leaf-abort)
(println "  leaf-park-drop     " d-leaf-drop)
(println "  ok-borrow          " d-ok " (control)")
(println "  err-plain          " d-plain " (control)")

(assert (%lt d-ok 50)
        (concat "control: a native tail call that COMPLETES runs its "
                "fall-through release, delta=" (number->string d-ok)))
(assert (%lt d-plain 50)
        (concat "control: a signal exit with no borrowed argument must release "
                "nothing, delta=" (number->string d-plain)))

(assert (%lt d-abort 50)
        (concat "a fiber-carrier exit strands the borrowed-arg retain, delta="
                (number->string d-abort)))
(assert (%lt d-heap 50)
        (concat "a fiber-carrier exit with a heap payload strands the "
                "borrowed-arg retain, delta=" (number->string d-heap)))
(assert (%lt d-restart 50)
        (concat "a restarted fiber must not re-run the consumed release, "
                "delta=" (number->string d-restart)))
(assert (%lt d-leaf-resume 50)
        (concat "a resumed io park on a leaf strands the argument retain, "
                "delta=" (number->string d-leaf-resume)))
(assert (%lt d-leaf-abort 50)
        (concat "an aborted io park on a leaf strands the argument retain, "
                "delta=" (number->string d-leaf-abort)))
(assert (%lt d-leaf-drop 50)
        (concat "a dropped io park on a leaf strands the argument retain, "
                "delta=" (number->string d-leaf-drop)))
(println "region-tail-signal-exit: ok")
