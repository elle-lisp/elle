(elle/epoch 13)
# audited: 2026-09-29
# A restart delivers into an error park, and owes what the raise site left unfunded: the leak gauge.
# docs/impl/region/park.md
#
# The delivery mints one reference for a restart into a `Call` site — a
# primitive, an instruction, a callee, a replayed frame, a refusal or abort
# over a primitive park, a parent a child's error stopped — and none for an
# `Emit` site, whose continuation funds its own. A missing mint is a
# use-after-free, which tests/impl/region-fiber-restart-uaf.lisp catches. A
# mint the continuation does not consume is a leak: one restart value's region
# per cycle, which this file gauges. The obvious fix, minting for every error
# park, strands the restart value of every `error` raise and of every abort at
# a yield — the two emit faces below.
#
# A fiber cancelled on an error is the other face here. The kill displaces the
# terminal error it finds, and that error's park retain has no other release
# once the slot is overwritten.
#
# This file is the LEAK gauge — an `arena/count` delta over a fixed window,
# which must be BOUNDED for every face.
#
# Each body uses the raising call's result directly as an element of the array
# it returns, the shape a program writes. The yield face is the control for
# that shape: a resume value in that position is released with the array.

(def window 400)

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

(defn raise-now []
  (+ 100 (error :boom)))
(defn yield-then-get []
  (yield 1)
  [:inner (get nil :x)])

# Each subject restarts with a fresh heap value and reads it back out of the
# fiber's result, so a region the restart strands is one per cycle.
(defn restart [f]
  (get (fiber/resume f @[:v]) 1))

# subjects — the raise sites ──────────────────────────────────────────────────

(defn w-emit []
  (let [f (fiber/new (fn [] [:got (error @[:boom])]) |:error|)]
    (fiber/resume f)
    (restart f)))

(defn w-primitive []
  (let [f (fiber/new (fn [] [:got (get nil :x)]) |:error|)]
    (fiber/resume f)
    (restart f)))

(defn w-instruction []
  (let [f (fiber/new (fn [] [:got (first nil)]) |:error|)]
    (fiber/resume f)
    (restart f)))

(defn w-tail []
  (let [f (fiber/new (fn [] (get nil :x)) |:error|)]
    (fiber/resume f)
    (fiber/resume f @[:v])))

(defn w-uncaught []
  (let [f (fiber/new (fn [] [:got (get nil :x)]) |:yield|)]
    (protect (fiber/resume f))
    (restart f)))

(defn w-callee []
  (let [f (fiber/new (fn [] [:got (raise-now)]) |:error|)]
    (fiber/resume f)
    (restart f)))

(defn w-replayed []
  (let [f (fiber/new (fn [] [:got (yield-then-get)]) |:yield :error|)]
    (fiber/resume f)
    (fiber/resume f)
    (restart f)))

(defn w-refused []
  (let [f (fiber/new (fn [] [:got (file/read "/no/such/path")]) |:fs :error|
                     :deny |:fs|)]
    (fiber/resume f)
    (fiber/refuse f @[:no])
    (restart f)))

(defn w-abort-denial []
  (let [f (fiber/new (fn [] [:got (file/read "/no/such/path")]) |:fs :error|
                     :deny |:fs|)]
    (fiber/resume f)
    (fiber/abort f @[:stop])
    (restart f)))

(defn w-abort-yield []
  (let [f (fiber/new (fn [] [:got (yield 1)]) |:yield :error|)]
    (fiber/resume f)
    (fiber/abort f @[:stop])
    (restart f)))

(defn w-escaped []
  (let [p (fiber/new (fn []
                       (let [c (fiber/new (fn [] (error @[:boom])) |:yield|)]
                         [:parent (fiber/resume c)])) |:error|)]
    (fiber/resume p)
    (restart p)))

# subjects — a cancel over an error ──────────────────────────────────────────

(defn w-cancel-caught []
  (let [f (fiber/new (fn [] [:got (+ 1 (error @[1 2 3]))]) |:error|)]
    (fiber/resume f)
    (fiber/cancel f @[:gone])
    (fiber/status f)))

(defn w-cancel-uncaught []
  (let [f (fiber/new (fn [] [:got (+ 1 (error @[1 2 3]))]) |:yield|)]
    (protect (fiber/resume f))
    (fiber/cancel f @[:gone])
    (fiber/status f)))

# controls ─────────────────────────────────────────────────────────────────────

# An error park nobody restarts: the fiber's free discharges it.
(defn c-dropped []
  (let [f (fiber/new (fn [] [:got (get nil :x)]) |:error|)]
    (fiber/resume f)
    3))

# A yield park answered by a resume: the delivery path an error park shares.
(defn c-yield []
  (let [f (fiber/new (fn [] [:got (yield 1)]) |:yield|)]
    (fiber/resume f)
    (restart f)))

(def faces
  [["control: an error park nobody restarts" c-dropped]
   ["a yield park answered by a resume" c-yield]
   ["an emit raise restarted" w-emit]
   ["a primitive raise restarted" w-primitive]
   ["an instruction raise restarted" w-instruction]
   ["a tail-position primitive raise restarted" w-tail]
   ["an uncaught raise restarted from :error" w-uncaught]
   ["a first-run callee raise restarted" w-callee]
   ["a raise in a replayed frame restarted" w-replayed]
   ["a refused denial restarted" w-refused]
   ["an abort over a denial park restarted" w-abort-denial]
   ["an abort at a yield restarted" w-abort-yield]
   ["a parent stopped by a child's error restarted" w-escaped]
   ["a caught error cancelled" w-cancel-caught]
   ["an uncaught error cancelled" w-cancel-uncaught]])

(def deltas
  (map (fn [face] [(get face 0) (measure (get face 1) 100 window)]) faces))

(println "region-fiber-restart deltas over " window " iters:")
(each r deltas
  (println "  " (get r 0) ": " (get r 1)))

# Every strand here is at least the restart value's own array per cycle, so a
# survivor reads ≥400 over the window. 100 is slack for the one-time intercept.
(each r deltas
  (assert (< (get r 1) 100)
          (concat (get r 0) " leaks, delta=" (number->string (get r 1)))))

# Value preservation: the restart value is what the fiber's result holds.
(assert (= @[:v] (w-primitive)) "a primitive restart lost its value")
(assert (= @[:v] (w-emit)) "an emit restart lost its value")
(assert (= @[:v] (w-abort-yield)) "an abort restart lost its value")
(assert (= :dead (w-cancel-caught)) "a cancelled error fiber is :dead")

(println "region-fiber-restart: ok")
