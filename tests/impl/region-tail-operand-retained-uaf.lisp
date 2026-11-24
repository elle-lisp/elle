(elle/epoch 18)
# audited: 2026-11-23
# The release of what a retained tail operand names frees nothing the callee, a later operand or the caller still reads.
# docs/impl/region/operand.md
#
# The soundness face of region-tail-operand-retained.lisp. A tail call retains
# a branch operand with a literal arm, so the frame's release of the fresh
# array `ps` runs before the call. Each witness gives the frame the only
# reference to `ps`, and reads it after that release. Under
# `--trace=guardfree` a freed page faults at the read.
#
# (r1) the callee reads `ps` whole on the arm that hands it over.
# (r2) the callee hands `ps` back, and the caller reads it.
# (r3) a later operand reads `ps` after the branch.
# (r4) the callee is a closure that captures `ps`, and reads it through the
#      capture on either arm.
# (r5) the arm hands over an element read out of `ps`.
# (r6) a native callee, whose fall-through runs the release behind the call.
# (r7) the `cond` of spec's `derived-of`, handed to stdlib `map`.

(def @yes true)
(def @no false)

(defn sum-from [xs i]
  (if (>= i (length xs))
    0
    (+ (get xs i) (sum-from xs (+ i 1)))))

(defn sum-all [xs]
  (sum-from xs 0))

(defn back [xs n]
  (if (= n 0) xs (back xs (- n 1))))

(defn sum-two [xs n]
  (+ (sum-all xs) n))

(defn r1 [c x]
  (let [ps @[x 10 20]]
    (sum-all (if c [7 8] ps))))

(defn r2 [c x]
  (let [ps @[x 10 20]]
    (back (if c [7 8] ps) 3)))

(defn r3 [c x]
  (let [ps @[x 10 20]]
    (sum-two (if c [7 8] ps) (sum-all ps))))

(defn r4 [c x]
  (let [ps @[x 10 20]]
    (letrec [g (fn [v n] (if (= n 0) (+ (sum-all v) (get ps 0)) (g v (- n 1))))]
      (g (if c [7 8] ps) 2))))

(defn r5 [c x]
  (let [ps @[@[x 10 20]]]
    (sum-all (if c [7 8] (get ps 0)))))

(defn r6 [c x]
  (let [ps @[x 10 20]]
    (length (if c [7 8] ps))))

(defn r7 [c x]
  (let [ps (if c @[] @[x])]
    (map (fn [t] [x t])
         (cond
           (nil? ps) []
           (not (empty? ps)) ps
           true [:unnamed]))))

(var i 0)
(while (< i 500)
  (assert (= (r1 no 1) 31) "r1: the callee reads ps whole")
  (assert (= (r1 yes 1) 15) "r1: the callee reads the literal")
  (assert (= (sum-all (r2 no 1)) 31)
          "r2: the caller reads ps the callee handed back")
  (assert (= (sum-all (r2 yes 1)) 15)
          "r2: the caller reads the literal handed back")
  (assert (= (r3 no 1) 62) "r3: the callee and a later operand read ps")
  (assert (= (r3 yes 1) 46) "r3: a later operand reads ps beside the literal")
  (assert (= (r4 no 1) 32) "r4: the callee reads ps as argument and capture")
  (assert (= (r4 yes 1) 16) "r4: the callee reads ps through its capture")
  (assert (= (r5 no 1) 31) "r5: the callee reads the element out of ps")
  (assert (= (r5 yes 1) 15) "r5: the callee reads the literal")
  (assert (= (r6 no 1) 3) "r6: the native callee reads ps")
  (assert (= (r6 yes 1) 2) "r6: the native callee reads the literal")
  (assert (= (r7 no 1) @[[1 1]]) "r7: map reads ps")
  (assert (= (r7 yes 1) [[1 :unnamed]]) "r7: map reads the literal")
  (assign i (+ i 1)))

(println "region-tail-operand-retained-uaf: ok")
