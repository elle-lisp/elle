(elle/epoch 15)
# audited: 2026-11-12
# A branch operand's arm that hands a tail call a fresh value releases what only a sibling arm hands over.
# docs/impl/region/operand.md
#
# The operand of a frame-replacing tail call is a branch. One arm hands the call
# a fresh pair that holds the accumulator by its own count, and another hands
# the call the accumulator itself. On the pair arm the frame still owes its
# release of the accumulator, so the arm runs it once the pair is built. The
# subjects are the list walker of stdlib `filter`, a copy of it as a named
# function, and the same operand spelled as a `cond` and as a `match`.
#
# One control takes the arm that hands the call the accumulator on every pass,
# where the release behind the call is the ownership move. The other is a native
# callee, whose fall-through runs the release behind the call on either arm.
#
# The counter-factual: the exemption took the union over the arms and left the
# release behind the `TailCall` on both paths, where the closure path never runs
# it. Each kept element stranded the accumulator it displaced.
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

# (a) the walker as a named function: a self tail call whose second operand
# keeps the element or passes the accumulator on.
(defn keep-if (c acc)
  (if (empty? c)
    (length acc)
    (keep-if (rest c) (if (< 0 (first c)) (pair (first c) acc) acc))))

# (b) stdlib `filter` over a list, whose `letrec` walker has the same operand.
(defn filter-all (xs)
  (length (filter (fn (x) (< 0 x)) xs)))

# (c) the same operand as a `cond`.
(defn keep-cond (c acc)
  (if (empty? c)
    (length acc)
    (keep-cond (rest c)
               (cond
                 (< 0 (first c)) (pair (first c) acc)
                 true acc))))

# (d) the same operand as a `match`.
(defn keep-match (c acc)
  (if (empty? c)
    (length acc)
    (keep-match (rest c)
                (match (< 0 (first c))
                  true (pair (first c) acc)
                  _ acc))))

# control: a native callee, whose fall-through runs the release behind the call.
(defn native-tail (x acc c)
  (length (if c (pair x acc) acc)))

(def keep-d (measure (fn () (keep-if positives ())) 20 window))
(def keep-mixed-d (measure (fn () (keep-if mixed ())) 20 window))
(def filter-d (measure (fn () (filter-all positives)) 20 window))
(def cond-d (measure (fn () (keep-cond positives ())) 20 window))
(def match-d (measure (fn () (keep-match positives ())) 20 window))
(def pass-d (measure (fn () (keep-if negatives ())) 20 window))
(def native-d (measure (fn () (native-tail 1 (list 2 3) true)) 20 window))
(def native-pass-d (measure (fn () (native-tail 1 (list 2 3) false)) 20 window))

(println "region-tail-operand-arm deltas over " window " iters:")
(println "  if, every element kept   " keep-d)
(println "  if, every other kept     " keep-mixed-d)
(println "  stdlib filter over a list " filter-d)
(println "  cond                     " cond-d)
(println "  match                    " match-d)
(println "  control: nothing kept    " pass-d)
(println "  control: native, pair    " native-d)
(println "  control: native, acc     " native-pass-d)

# Each strand is at least one object per call, so a surviving one reads at
# least the window. 100 is slack for the one-time intercept.
(defn bounded? (d label)
  (assert (%lt d 100) (concat label " leaks, delta=" (number->string d))))

(bounded? keep-d "the accumulator an if arm displaces, every element kept")
(bounded? keep-mixed-d "the accumulator an if arm displaces, every other kept")
(bounded? filter-d "the accumulator stdlib filter's list walker displaces")
(bounded? cond-d "the accumulator a cond body displaces")
(bounded? match-d "the accumulator a match arm displaces")
(bounded? pass-d "control: the accumulator an arm hands the call")
(bounded? native-d "control: a native callee handed the pair")
(bounded? native-pass-d "control: a native callee handed the accumulator")

# Value preservation: a release that moves must not change what runs.
(assert (= (keep-if positives ()) 4) "keep-if result lost")
(assert (= (keep-if mixed ()) 2) "keep-if mixed result lost")
(assert (= (filter-all mixed) 2) "filter result lost")
(assert (= (keep-cond positives ()) 4) "keep-cond result lost")
(assert (= (keep-match positives ()) 4) "keep-match result lost")
(assert (= (keep-if negatives ()) 0) "keep-if pass result lost")
(assert (= (native-tail 1 (list 2 3) true) 3) "native-tail pair result lost")
(assert (= (native-tail 1 (list 2 3) false) 2) "native-tail acc result lost")

(println "region-tail-operand-arm: ok")
