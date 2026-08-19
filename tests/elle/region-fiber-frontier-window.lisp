(elle/epoch 12)
# A fiber crossing is a counted holder too
# (docs/impl/region/mechanism.md § "A fiber crossing is a counted holder too";
# docs/impl/region/owner.md § "A resume value crosses counted, or not at all").
#
# The branch-arm release window and the lowerer's frame-exit release both make a
# release fire on a path where none fired before, so both are admitted only where
# this frame holds the region's one reference. What that admission needs from
# escape is narrower than "escapes": an UNCOUNTED second holder. Every seam that
# hands a value to another fiber counts a reference of its own — the park's
# `EmitEscape` retain going out, the resume value's own mint coming back, and
# `chan/send`'s send-site incref — so the fiber facet rides along and only the
# CONTAINMENT facets (store, and capture by a closure that itself escapes) refuse.
#
# What refusing the fiber facet costs is a whole owned parameter per call on every
# path that does not cross. The production shape is a scheduler's wake step: it
# receives the completed fiber by tail-call move and resumes a waiter with it, so
# the release sits inside the arm that finds a waiter — and an ordinary program,
# with no waiter outstanding, never takes that arm. `wake-empty` below is that
# shape; `wake-one` and `wake-two` are the same call on the paths that do deliver.
#
# This file is the LEAK gauge — an `arena/count` delta over a fixed window, which
# must be BOUNDED for each crossing. The soundness complement is
# region-fiber-frontier-window-uaf.lisp; the per-op rate through the real
# scheduler is the `spawn-join` probe in tests/elle/oracle.lisp.

(def window 2000)

(defn measure (thunk warm window)
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

# A fiber parked on its first `yield`, waiting for a value to arrive as the resume
# result. Reading the value proves the delivery survived the resumer's release.
(defn waiter-body ()
  (let [x (yield 0)]
    (length (first x))))

# (a) the install face, and the production shape: an owned parameter the frame
# receives by tail-call move and delivers to each waiter. `ws` decides whether the
# delivering arm runs at all, and no answer may strand `v`.
(defn wake-all (ws v)
  (each w in ws
    (fiber/resume w v)))
(defn wake-empty (v)
  (wake-all @[] v))
(defn wake-one (v)
  (let [w (fiber/new waiter-body |:yield|)]
    (fiber/resume w)
    (wake-all (array w) v)))
(defn wake-two (v)
  (let [a (fiber/new waiter-body |:yield|)
        b (fiber/new waiter-body |:yield|)]
    (fiber/resume a)
    (fiber/resume b)
    (wake-all (array a b) v)))

# (b) the emit face: one arm yields the live-in parameter to the resumer. Driven
# through both arms — the non-yielding one is where the release is new, and the
# yielding one must still hand over exactly the delivery reference.
(defn arm-yield (v t)
  (match t
    :a (length v)
    _ (begin
        (yield v)
        0)))
(defn drive-yield-short (v)
  (let [f (fiber/new (fn () (arm-yield v :a)) |:yield|)]
    (fiber/resume f)))
(defn drive-yield-cross (v)
  (let [f (fiber/new (fn () (arm-yield v :z)) |:yield|)]
    (fiber/resume f)
    (fiber/resume f)))

# (c) the resume value a body binds and keeps across a further park — the shape the
# mint exists for. Without a reference of its own the body reads the resumer's, and
# the resumer's release frees it under the second read.
(defn keeper-body ()
  (let [x (yield 0)]
    (yield (length (first x)))
    (length (first x))))
(defn deliver-and-keep (v)
  (let [w (fiber/new keeper-body |:yield|)]
    (fiber/resume w)
    (wake-all (array w) v)
    (fiber/resume w)))

# controls ─────────────────────────────────────────────────────────────────────
# The same shapes with no crossing at all: already bounded, so a red subject above
# is the facet and not the surrounding shape.
(defn ctl-all (ws v)
  (each w in ws
    (length v)))
(defn ctl-empty (v)
  (ctl-all @[] v))
(defn ctl-arm (v t)
  (match t
    :a (length v)
    _ 0))

(def wake-empty-d (measure (fn () (wake-empty (list "a" "b"))) 200 window))
(def wake-one-d (measure (fn () (wake-one (list "a" "b"))) 200 window))
(def wake-two-d (measure (fn () (wake-two (list "a" "b"))) 200 window))
(def keep-d (measure (fn () (deliver-and-keep (list "a" "b"))) 200 window))
(def yield-short-d
  (measure (fn () (drive-yield-short (list "a" "b"))) 200 window))
(def yield-cross-d
  (measure (fn () (drive-yield-cross (list "a" "b"))) 200 window))
(def ctl-empty-d (measure (fn () (ctl-empty (list "a" "b"))) 200 window))
(def ctl-arm-d (measure (fn () (ctl-arm (list "a" "b") :a)) 200 window))

(println "region-fiber-frontier-window deltas over " window " iters:")
(println "  install: no waiter " wake-empty-d "  one waiter " wake-one-d
         "  two waiters " wake-two-d "  kept across a park " keep-d)
(println "  emit: non-yielding arm " yield-short-d "  yielding arm "
         yield-cross-d)
(println "  controls: no crossing " ctl-empty-d "  plain arm " ctl-arm-d)

# Every leak in this class is at least one whole region per call, so a surviving
# strand reads ≥2000 over the window. 100 is slack for the one-time intercept.
(defn bounded? (d label)
  (assert (%lt d 100) (concat label " leaks, delta=" (number->string d))))

(bounded? ctl-empty-d "control: the same walk with no crossing")
(bounded? ctl-arm-d "control: the same branch with no crossing")
(bounded? wake-empty-d "owned parameter delivered to no waiter")
(bounded? wake-one-d "owned parameter delivered to one waiter")
(bounded? wake-two-d "owned parameter delivered to two waiters")
(bounded? keep-d "delivered value a body keeps across a further park")
(bounded? yield-short-d "live-in parameter a sibling arm yields")
(bounded? yield-cross-d "live-in parameter the taken arm yields")

# Value preservation: admitting the facet must not change what runs.
(assert (= (wake-empty (list "a" "b")) nil) "wake-empty result changed")
(assert (= (wake-one (list "a" "b")) nil) "wake-one result changed")
(assert (= (deliver-and-keep (list "a" "b")) 1) "kept value lost")
(assert (= (drive-yield-short (list "a" "b")) 2) "yield short arm result lost")
(assert (= (drive-yield-cross (list "a" "b")) 0)
        "yield crossing arm result lost")

(println "region-fiber-frontier-window: ok")
