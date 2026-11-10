(elle/epoch 15)
# audited: 2026-11-10
# A value a `let` binds and a branch's arms hand to a native is released before the frame-replacing tail call the branch feeds.
# docs/impl/region/operand.md
#
# The operand of a frame-replacing tail call is a `let` whose body is a branch,
# and each arm hands the bound value to a native whose fresh result is the arm's
# value. The callee receives that result and nothing of the bound value, so the
# frame owes its release before the call. The two subjects are the shapes a
# worker spawn takes: an array the arms read through a native, and the closure
# to run, bound once, handed to a spawn in each arm, with the handle going to
# the join that replaces the frame. The control hands the bound value itself to
# the callee, where the release is the ownership move and stays behind the call.
#
# The counter-factual: the exemption read every child of the `let`, found the
# bound value's allocation in its binding, and left the release in the dead
# block, so each call stranded the array, or the closure it spawned.
#
# This file is the LEAK gauge, an `arena/count` delta over a fixed window that
# must stay bounded on every path. The soundness complement is
# region-tail-operand-let-uaf.lisp.

(def window 2000)

# The branch must stay a branch: a condition the fold cannot decide.
(def flag (> (length (sys/argv)) 100))

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

# A callee whose body no graft replaces, so the call to it replaces the frame.
(defn sink (h)
  (block :keep
    (length h)))

# (a) the array the arms read through `string`, a Fresh native, in an operand
# whose value is each arm's fresh list.
(defn arms-read (x c)
  (sink (let [a (array x x)]
          (if c (list (string a)) (list (string a) 0)))))

# (b) the worker spawn: the closure handed to a spawn in each arm, and the
# handle handed to the join, a stdlib closure, that replaces the frame.
(defn spawn-join (x c)
  (os/join (let [closure (fn [] x)]
             (if c (os/spawn-vm closure) (os/spawn-vm closure)))))

# control: each arm hands the callee the bound value itself, so the release is
# the ownership move and the callee's owned-parameter release consumes it.
(defn arms-move (x c)
  (sink (let [a (array x x)]
          (if c a a))))

(def arms-read-d (measure (fn () (arms-read 1 flag)) 200 window))
(def arms-read-then-d (measure (fn () (arms-read 1 (not flag))) 200 window))
(def spawn-join-d (measure (fn () (spawn-join 1 flag)) 50 200))
(def arms-move-d (measure (fn () (arms-move 1 flag)) 200 window))

(println "region-tail-operand-let deltas over " window " iters:")
(println "  arms read the array: else " arms-read-d "  then " arms-read-then-d)
(println "  spawn and join over 200 iters: " spawn-join-d)
(println "  control: arms move the array " arms-move-d)

# Every leak here is at least one whole region per call, so a surviving strand
# reads at least the window. 100 is slack for the one-time intercept.
(defn bounded? (d label)
  (assert (%lt d 100) (concat label " leaks, delta=" (number->string d))))

(bounded? arms-read-d "the array both arms read through a native, else arm")
(bounded? arms-read-then-d "the array both arms read through a native, then arm")
(bounded? spawn-join-d
          "the closure each arm spawns, joined by a frame-replacing callee")
(bounded? arms-move-d "control: the array an arm hands the callee")

# Value preservation: a release that moves must not change what runs.
(assert (= (arms-read 1 flag) 2) "arms-read else arm result lost")
(assert (= (arms-read 1 (not flag)) 1) "arms-read then arm result lost")
(assert (= (spawn-join 7 flag) 7) "spawn-join result lost")
(assert (= (arms-move 1 flag) 2) "arms-move result lost")

(println "region-tail-operand-let: ok")
