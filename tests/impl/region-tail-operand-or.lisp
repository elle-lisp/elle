(elle/epoch 15)
# audited: 2026-11-13
# An `and` or `or` operand of a tail call releases, on the edge it stops at, what only a later operand hands over.
# docs/impl/region/operand.md
#
# The operand of a frame-replacing tail call is an `and` or an `or`, whose value
# is the operand it stopped at. Each operand is an arm of the tail operand, and
# its end is the edge on which it stops. Where the call receives the accumulator
# through a later operand alone, the frame owes its release on that edge.
#
# (a) an `or` that keeps the pair an `and` builds, or passes the accumulator on.
# (b) the same with an `if` as the first `or` operand.
# (c) an `and` that hands over the accumulator, or stops at `false` and drops it.
#
# The controls take the operand that hands the call the accumulator on every
# pass, where the release behind the call is the ownership move.
#
# The counter-factual: a short-circuit operand opened no arm point, so the
# release stayed behind the `TailCall` on every path, where the closure path
# never runs it. Each pass that stopped early stranded the accumulator.
#
# This file is the LEAK gauge, an `arena/count` delta over a fixed window that
# must stay bounded on every path. The soundness complement is
# region-tail-operand-arm-uaf.lisp.

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

(defn keep-or (c acc)
  (if (empty? c)
    (length acc)
    (keep-or (rest c) (or (and (< 0 (first c)) (pair (first c) acc)) acc))))

(defn keep-or-if (c acc)
  (if (empty? c)
    (length acc)
    (keep-or-if (rest c) (or (if (< 0 (first c)) (pair (first c) acc) nil) acc))))

(defn and-walk (c acc)
  (if (empty? c)
    (if acc (length acc) 0)
    (and-walk (rest c) (and (< 0 (first c)) acc))))

(def or-d (measure (fn () (keep-or positives ())) 20 window))
(def or-if-d (measure (fn () (keep-or-if positives ())) 20 window))
(def and-d (measure (fn () (and-walk (list -1) (list 1 2))) 20 window))
(def or-pass-d (measure (fn () (keep-or negatives ())) 20 window))
(def and-pass-d (measure (fn () (and-walk (list 1) (list 1 2))) 20 window))

(println "region-tail-operand-or deltas over " window " iters:")
(println "  or of and, every kept    " or-d)
(println "  or of if, every kept     " or-if-d)
(println "  and, accumulator dropped " and-d)
(println "  control: or, none kept   " or-pass-d)
(println "  control: and, handed on  " and-pass-d)

# Each strand is at least one object per call, so a surviving one reads at
# least the window. 100 is slack for the one-time intercept.
(defn bounded? (d label)
  (assert (%lt d 100) (concat label " leaks, delta=" (number->string d))))

(bounded? or-d "the accumulator an or of an and displaces")
(bounded? or-if-d "the accumulator an or of an if displaces")
(bounded? and-d "the accumulator an and drops")
(bounded? or-pass-d "control: the accumulator an or hands the call")
(bounded? and-pass-d "control: the accumulator an and hands the call")

# Value preservation: a release that moves must not change what runs.
(assert (= (keep-or mixed ()) 2) "keep-or result lost")
(assert (= (keep-or-if mixed ()) 2) "keep-or-if result lost")
(assert (= (and-walk (list 1 2) (list 1 2)) 2) "and-walk result lost")
(assert (= (and-walk (list 1 -2) (list 1 2)) 0) "and-walk drop result lost")

(println "region-tail-operand-or: ok")
