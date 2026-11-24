(elle/epoch 18)
# audited: 2026-11-23
# A tail-call operand the call retains releases what it names before the call, on every arm.
# docs/impl/region/operand.md
#
# The operand of a frame-replacing tail call is a branch with a literal arm.
# The literal is a constant the frame does not own, so the call retains the
# branch's value whichever arm runs, and the callee's release consumes the
# retain. The fresh value `ps` that another arm hands over is the frame's own,
# and the call moves none of its references, so its release runs before the
# call. The subjects are an `if` on each arm, the `cond` of spec's
# `derived-of`, and the same `if` handed to stdlib `map`.
#
# One control hands the call a fresh value on the other arm instead, so the
# operand takes the move and the arm point releases `ps`. The other is a
# native callee, whose fall-through runs the release behind the call.
#
# The counter-factual: the exemption read the union over the arms as if the
# operand took the move, and left `ps`'s release behind the `TailCall`, where
# the closure path never runs it. Each call stranded `ps` on both arms.
#
# This file is the LEAK gauge, an `arena/region-count` delta over a fixed
# window that must stay bounded on every path. The soundness complement is
# region-tail-operand-retained-uaf.lisp.

(def window 500)

# Mutable, so no call below folds its branch away.
(def @yes true)
(def @no false)

(include-file "gauge/window.lisp")

(defn walk-down [xs]
  (if (empty? xs) nil (walk-down (rest xs))))

# (a) an `if` whose other arm is a literal.
(defn hand-over [c]
  (let [ps @[1]]
    (walk-down (if c [7 8] ps))))

# (b) the `cond` of `derived-of`: an empty `ps` falls through to a literal.
(defn cond-over [c]
  (let [ps (if c @[] @[1])]
    (walk-down (cond
                 (nil? ps) []
                 (not (empty? ps)) ps
                 true [:unnamed]))))

# (c) the same `if` handed to stdlib `map`, a closure the lowerer does not
# inline.
(defn map-over [c]
  (let [ps @[1]]
    (map (fn [t] t) (if c [:unnamed] ps))))

# control: a fresh value on the other arm, so the operand takes the move.
(defn fresh-other [c]
  (let [ps @[1]]
    (walk-down (if c @[] ps))))

# control: a native callee, whose fall-through runs the release behind the call.
(defn native-over [c]
  (let [ps @[1]]
    (length (if c [7 8] ps))))

(def literal-d (measure-regions (fn [] (hand-over yes)) 20 window))
(def handed-d (measure-regions (fn [] (hand-over no)) 20 window))
(def cond-literal-d (measure-regions (fn [] (cond-over yes)) 20 window))
(def cond-handed-d (measure-regions (fn [] (cond-over no)) 20 window))
(def map-literal-d (measure-regions (fn [] (map-over yes)) 20 window))
(def map-handed-d (measure-regions (fn [] (map-over no)) 20 window))
(def fresh-d (measure-regions (fn [] (fresh-other yes)) 20 window))
(def fresh-handed-d (measure-regions (fn [] (fresh-other no)) 20 window))
(def native-d (measure-regions (fn [] (native-over yes)) 20 window))
(def native-handed-d (measure-regions (fn [] (native-over no)) 20 window))

(println "region-tail-operand-retained deltas over " window " iters:")
(println "  if, literal arm          " literal-d)
(println "  if, ps arm               " handed-d)
(println "  cond, literal arm        " cond-literal-d)
(println "  cond, ps arm             " cond-handed-d)
(println "  map, literal arm         " map-literal-d)
(println "  map, ps arm              " map-handed-d)
(println "  control: fresh, other    " fresh-d)
(println "  control: fresh, ps arm   " fresh-handed-d)
(println "  control: native, literal " native-d)
(println "  control: native, ps arm  " native-handed-d)

# Each strand is one region per call, so a surviving one reads the window. 100
# is slack for the one-time intercept.
(def bounded? (bounded-by 100))

(bounded? literal-d "ps beside a literal arm that runs")
(bounded? handed-d "ps on the arm that hands it to the call")
(bounded? cond-literal-d "ps beside a cond's literal clause that runs")
(bounded? cond-handed-d "ps on the cond clause that hands it to the call")
(bounded? map-literal-d "ps beside a literal arm, handed to map")
(bounded? map-handed-d "ps handed to map")
(bounded? fresh-d "control: ps beside a fresh arm")
(bounded? fresh-handed-d "control: ps moved to the call")
(bounded? native-d "control: a native callee handed the literal")
(bounded? native-handed-d "control: a native callee handed ps")

# Value preservation: a release that moves must not change what runs.
(assert (nil? (hand-over true)) "hand-over literal result lost")
(assert (nil? (hand-over false)) "hand-over ps result lost")
(assert (= (map-over true) [:unnamed]) "map-over literal result lost")
(assert (= (map-over false) @[1]) "map-over ps result lost")
(assert (= (native-over true) 2) "native-over literal result lost")
(assert (= (native-over false) 1) "native-over ps result lost")

(println "region-tail-operand-retained: ok")
