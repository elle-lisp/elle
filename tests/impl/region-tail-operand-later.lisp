(elle/epoch 15)
# audited: 2026-11-13
# A branch operand's arm postpones to the tail call the release a later operand or the callee still reads.
# docs/impl/region/operand.md
#
# The operand of a frame-replacing tail call is a branch. One arm hands the call
# a fresh pair that holds the accumulator by its own count, and another hands
# the call the accumulator itself. A later operand, or the callee, reads the
# accumulator after the arm. So on the pair arm the frame owes its release of
# the accumulator, and that release has to wait until the call's operands and
# callee are evaluated. The arm copies the release's slot, and the instruction
# just before the `TailCall` runs the release.
#
# The subjects read the accumulator in a later operand, through a name the
# operand's `let` binds, in a later operand after an `or`, and in the callee.
# One more pauses the walk on a fuel budget at each charge it makes and drops
# the fiber, so the walk over the abandoned frame runs what the frame owed.
#
# One control takes the arm that hands the call the accumulator on every pass,
# where the release behind the call is the ownership move. Another is a native
# callee, whose fall-through runs the release behind the call on either arm.
# The last pauses and drops a walker with no later operand.
#
# The counter-factual: the arm refused a region a later operand reads, and the
# release stayed behind the `TailCall`, where the closure path never runs it.
# Each kept element stranded the accumulator it displaced.
#
# This file is the LEAK gauge, an `arena/count` delta over a fixed window that
# must stay bounded on every path. The soundness complement is the `d*`
# witnesses of region-tail-operand-arm-uaf.lisp.

(def window 500)

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

(def positives (list 1 2 3 4))
(def negatives (list -1 -2 -3 -4))
(def mixed (list 1 -2 3 -4))

# (a) a later operand reads the accumulator.
(defn keep-later (c acc n)
  (if (empty? c)
    (+ n (length acc))
    (keep-later (rest c) (if (< 0 (first c)) (pair (first c) acc) acc)
                (length acc))))

# (b) the later operand reads it through a name the `let` binds.
(defn keep-alias (c acc n)
  (if (empty? c)
    (+ n (length acc))
    (let [b acc]
      (keep-alias (rest c) (if (< 0 (first c)) (pair (first c) acc) acc)
                  (length b)))))

# (c) an `or` keeps the pair an `and` builds or passes the accumulator on, and
# a later operand reads the accumulator.
(defn keep-or-later (c acc n)
  (if (empty? c)
    (+ n (length acc))
    (keep-or-later (rest c) (or (and (< 0 (first c)) (pair (first c) acc)) acc)
                   (length acc))))

# (d) the callee reads the accumulator: `pick` answers the walker itself.
(defn pick (acc)
  (if (empty? acc) keep-callee keep-callee))

(defn keep-callee (c acc)
  (if (empty? c)
    (length acc)
    ((pick acc) (rest c) (if (< 0 (first c)) (pair (first c) acc) acc))))

# (e) the walk of (a) on a fiber whose fuel budget pauses it at each charge it
# makes, dropped while it waits. The walk spends 31 units over `positives`.
(defn paused-walk (walk budget)
  (let [f (fiber/new walk |:fuel|)]
    (fiber/set-fuel f budget)
    (fiber/resume f)
    (fiber/status f)))

(defn paused-walks (walk)
  (var k 1)
  (while (%lt k 32)
    (paused-walk walk k)
    (assign k (%add k 1))))

# control: a walker with no later operand, paused and dropped the same way.
(defn keep-if (c acc)
  (if (empty? c)
    (length acc)
    (keep-if (rest c) (if (< 0 (first c)) (pair (first c) acc) acc))))

# control: a native callee, whose fall-through runs the release behind the call.
(defn native-later (x acc c)
  (list (if c (pair x acc) acc) (length acc)))

(def later-d (measure (fn () (keep-later positives () 0)) 20 window))
(def later-mixed-d (measure (fn () (keep-later mixed () 0)) 20 window))
(def alias-d (measure (fn () (keep-alias positives () 0)) 20 window))
(def or-d (measure (fn () (keep-or-later positives () 0)) 20 window))
(def callee-d (measure (fn () (keep-callee positives ())) 20 window))
(def paused-d
  (measure (fn () (paused-walks (fn () (keep-later positives () 0)))) 2 50))
(def pass-d (measure (fn () (keep-later negatives () 0)) 20 window))
(def paused-if-d
  (measure (fn () (paused-walks (fn () (keep-if positives ())))) 2 50))
(def native-d (measure (fn () (native-later 1 (list 2 3) true)) 20 window))
(def native-pass-d (measure (fn () (native-later 1 (list 2 3) false)) 20 window))

(println "region-tail-operand-later deltas over " window " iters:")
(println "  later operand, every kept " later-d)
(println "  later operand, half kept  " later-mixed-d)
(println "  read through a let name   " alias-d)
(println "  or, then a later operand  " or-d)
(println "  the callee reads it       " callee-d)
(println "  paused and dropped (x50)  " paused-d)
(println "  control: nothing kept     " pass-d)
(println "  control: no later operand " paused-if-d)
(println "  control: native, pair     " native-d)
(println "  control: native, acc      " native-pass-d)

# Each strand is at least one object per call, so a surviving one reads at
# least the window. 100 is slack for the one-time intercept.
(defn bounded? (d label)
  (assert (%lt d 100) (concat label " leaks, delta=" (number->string d))))

(bounded? later-d "the accumulator a later operand reads, every element kept")
(bounded? later-mixed-d
          "the accumulator a later operand reads, every other element kept")
(bounded? alias-d "the accumulator a later operand reads through a let name")
(bounded? or-d "the accumulator an or displaces and a later operand reads")
(bounded? callee-d "the accumulator the callee reads")
(bounded? paused-d "a walk a fuel pause abandons")
(bounded? pass-d "control: the accumulator an arm hands the call")
(bounded? paused-if-d "control: a walk with no later operand, abandoned")
(bounded? native-d "control: a native callee handed the pair")
(bounded? native-pass-d "control: a native callee handed the accumulator")

# Value preservation: a release that waits for the call must not change what
# runs.
(assert (= (keep-later positives () 0) 7) "keep-later result lost")
(assert (= (keep-later mixed () 0) 4) "keep-later mixed result lost")
(assert (= (keep-alias positives () 0) 7) "keep-alias result lost")
(assert (= (keep-or-later mixed () 0) 4) "keep-or-later result lost")
(assert (= (keep-callee mixed ()) 2) "keep-callee result lost")
(assert (= (keep-later negatives () 0) 0) "keep-later pass result lost")
(assert (= (length (native-later 1 (list 2 3) true)) 2)
        "native-later pair result lost")
(assert (= (native-later 1 (list 2 3) false) (list (list 2 3) 2))
        "native-later acc result lost")

(println "region-tail-operand-later: ok")
