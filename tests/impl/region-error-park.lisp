(elle/epoch 15)
# audited: 2026-10-06
# A callee's error parks its chain, and every seam that ends the park runs what the frames owed: the leak gauge.
# docs/impl/region/unwind.md
#
# An error parks every frame from the raise to the fiber body. Each callee here
# holds a heap value live across the raise, so the parked chain owes that
# value's release. A restart runs the frames to completion. A fiber released
# with the chain parked, a cancel, and a host that discards the chain each end
# the park without running it, so each must run what the frames owed.
#
# This file is the LEAK gauge: `arena/count` and `arena/region-count` deltas
# over a fixed window, each beside a control: the same chain parked on a
# yield, or a raise through the same route holding nothing. The soundness face
# is region-error-park-uaf.lisp.
#
# The value assertions run first. A raise that abandons the callee makes a
# restart answer the body's call to it, so they fail on the value before any
# gauge is read.

(def window 300)

(defn measure [thunk warm window]
  (var i 0)
  (while (%lt i warm)
    (thunk)
    (assign i (%add i 1)))
  (def objects (arena/count))
  (def regions (arena/region-count))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  [(%sub (arena/count) objects) (%sub (arena/region-count) regions)])

# Two frames, each holding a string live across the raise below it.
(defn holder [n]
  (let [h (string "h" n)]
    [(get nil :x) h]))
(defn outer-holder [n]
  (let [o (string "o" n)]
    [(holder n) o]))

# The same chain, parked on a yield.
(defn yield-holder [n]
  (let [h (string "h" n)]
    [(yield 1) h]))
(defn yield-outer [n]
  (let [o (string "o" n)]
    [(yield-holder n) o]))

(defn raising []
  (fiber/new (fn [] [:got (outer-holder 1)]) |:error|))
(defn yielding []
  (fiber/new (fn [] [:got (yield-outer 1)]) |:yield|))

# values ───────────────────────────────────────────────────────────────────────

(let [f (raising)]
  (assert (= :type-error (get (fiber/resume f) :error)) "the callee raises")
  (assert (= [:got [[7 "h1"] "o1"]] (fiber/resume f 7))
          "the restart answers the get inside the callee"))

(let [f (fiber/new (fn [] [:got (eval '(oh 1) {'oh outer-holder})]) |:error|)]
  (assert (= :eval-error (get (fiber/resume f) :error))
          "eval reports the hosted error")
  (assert (= [:got 7] (fiber/resume f 7)) "a restart answers the eval call"))

(let [f (fiber/new (fn [] [:got (arena/allocs (fn [] (outer-holder 1)))])
                   |:error|)]
  (assert (= :type-error (get (fiber/resume f) :error))
          "arena/allocs passes the thunk's error on")
  (assert (= [:got 7] (fiber/resume f 7))
          "a restart answers the arena/allocs call"))

# subjects ─────────────────────────────────────────────────────────────────────

# (a) the chain restarted to completion.
(defn w-restart []
  (let [f (raising)]
    (fiber/resume f)
    (fiber/resume f 7)))
(defn c-restart []
  (let [f (yielding)]
    (fiber/resume f)
    (fiber/resume f 7)))

# (b) the fiber released with the chain parked.
(defn w-dropped []
  (let [f (raising)]
    (fiber/resume f)
    3))
(defn c-dropped []
  (let [f (yielding)]
    (fiber/resume f)
    3))

# (c) the same, through `protect`, whose fiber is gone when it returns.
(defn w-protect []
  (protect (outer-holder 1)))

# (d) a cancel over the parked chain.
(defn w-cancel []
  (let [f (raising)]
    (fiber/resume f)
    (fiber/cancel f @[:gone])))
(defn c-cancel []
  (let [f (yielding)]
    (fiber/resume f)
    (fiber/cancel f @[:gone])))

# (e) a host discards the chain: `eval`, and `arena/allocs` running a thunk.
(defn w-eval []
  (protect (eval '(oh 1) {'oh outer-holder})))
(defn w-allocs []
  (protect (arena/allocs (fn [] (outer-holder 1)))))

# The control for the protect and host faces raises through the same route
# holding nothing, so what the host does with the error itself cancels out.
(defn bare [n]
  [(get nil :x)])
(defn c-protect []
  (protect (bare 1)))
(defn c-eval []
  (protect (eval '(oh 1) {'oh bare})))
(defn c-allocs []
  (protect (arena/allocs (fn [] (bare 1)))))

# measurement ──────────────────────────────────────────────────────────────────

(def faces
  [["a restarted chain" w-restart c-restart]
   ["a dropped chain" w-dropped c-dropped]
   ["a chain under protect" w-protect c-protect]
   ["a cancelled chain" w-cancel c-cancel]
   ["a chain eval discards" w-eval c-eval]
   ["a chain arena/allocs discards" w-allocs c-allocs]])

(def slack 50)

(each face faces
  (let [[label subject control] face
        d (measure subject 20 window)
        c (measure control 20 window)
        objects (- (get d 0) (get c 0))
        regions (- (get d 1) (get c 1))]
    (println "  " label " " d " control " c)
    (assert (< objects slack)
            (concat label ": objects beyond the control, delta="
                    (number->string objects)))
    (assert (< regions slack)
            (concat label ": regions beyond the control, delta="
                    (number->string regions)))))

(println "region-error-park: ok")
