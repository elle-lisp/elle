(elle/epoch 15)
# audited: 2026-11-13
# A release a branch operand's arm runs frees nothing the tail callee or a later operand still reads.
# docs/impl/region/operand.md
#
# The soundness face of region-tail-operand-arm.lisp. An arm of a tail call's
# branch operand releases what only a sibling arm hands the call. Each witness
# below gives the frame the only reference to the accumulator, through a tail
# call from its driver, and then reads it after the arm has run. Under
# `--trace=guardfree` a freed page faults at the read.
#
# (u1) the pair arm: the pair holds the accumulator by its own count, and the
#      callee walks the whole list the pairs built.
# (u2) the accumulator arm: the call moves the accumulator itself, and the
#      callee reads it.
# (u3) a native callee: its fall-through runs the release behind the call, and
#      on the pair arm that release must find the slot the arm stamped.
# (u4) a later operand reads the accumulator through another name, after an arm
#      whose value does not hold it. A release in the arm frees what that read
#      reaches, so the arm runs none.
# (u5) a pair the operand's `let` binds ahead of its branch: the arm that hands
#      the call the pair moves it, and the sibling arm releases it.
# (u6) an arm whose value is the tail of the list the `let` binds, a cell in
#      the list's own region. That arm names the region, so it runs no release.
# (u7) a native callee handed the bound pair or the accumulator: its
#      fall-through runs the release behind the call, which must find the slot
#      the sibling arm stamped.
# (u8) an `or` that keeps the pair an `and` builds or passes the accumulator
#      on: the `and`'s stopping edge releases it, and the pair holds it.
# (u9) an `or` whose first operand is an `if` with a `nil` arm: that arm runs
#      before the `or` tests it and goes on to hand over the accumulator, so it
#      releases nothing.
# (u10) an `or` whose later operand reads the accumulator through another
#      name, after a first operand that stops with a fresh list.
#
# The `d*` witnesses read the accumulator after an arm that postpones its
# release to the call: the release runs just before the `TailCall`, once the
# read is done.
#
# (d1) a self tail call whose later operand reads the accumulator whole.
# (d2) a callee chosen by a call that reads the accumulator whole.
# (d3) a native callee on either arm, beside a later operand that reads the
#      accumulator. Its fall-through runs the release behind the call, which
#      must find the slot the postponed release stamped.
# (d4) the walk of (d1) paused by its fuel budget at each charge it makes, then
#      dropped. The walk over the abandoned frame runs the release once, so the
#      driver still reads the list it handed the walk.

(defn sum (xs)
  (if (empty? xs)
    0
    (+ (first xs) (sum (rest xs)))))

(defn keep-if (c acc)
  (if (empty? c)
    (sum acc)
    (keep-if (rest c) (if (< 0 (first c)) (pair (first c) acc) acc))))

(defn native-tail (x acc c)
  (length (if c (pair x acc) acc)))

(defn sink (xs n)
  (block :keep
    (+ (sum xs) n)))

(defn later (c acc)
  (let [b acc]
    (sink (if (< 0 (first c)) (list 9) acc) (length b))))

(defn keep-bound (c acc)
  (if (empty? c)
    (sum acc)
    (keep-bound (rest c)
                (let [t (pair (first c) acc)]
                  (if (< 0 (first c)) t acc)))))

(defn tail-of-bound (c acc)
  (if (empty? c)
    (sum acc)
    (tail-of-bound (rest c)
                   (let [t (list (first c) 10 20)]
                     (if (< 0 (first c)) t (rest t))))))

(defn native-bound (x acc c)
  (length (let [t (pair x acc)]
            (if c t acc))))

(defn keep-or (c acc)
  (if (empty? c)
    (sum acc)
    (keep-or (rest c) (or (and (< 0 (first c)) (pair (first c) acc)) acc))))

(defn or-if (c acc)
  (if (empty? c)
    (sum acc)
    (or-if (rest c) (or (if (< 0 (first c)) nil (pair (first c) acc)) acc))))

(defn later-or (c acc)
  (let [b acc]
    (sink (or (and (< 0 (first c)) (list 9)) acc) (length b))))

(defn keep-later (c acc n)
  (if (empty? c)
    (+ (sum acc) n)
    (keep-later (rest c) (if (< 0 (first c)) (pair (first c) acc) acc) (sum acc))))

(defn pick (acc)
  (if (< (sum acc) 0) keep-callee keep-callee))

(defn keep-callee (c acc)
  (if (empty? c)
    (sum acc)
    ((pick acc) (rest c) (if (< 0 (first c)) (pair (first c) acc) acc))))

(defn native-later (x acc c)
  (list (if c (pair x acc) acc) (sum acc)))

(defn paused (base budget)
  (let [f (fiber/new (fn () (keep-later (list 1 -2 3) base 0)) |:fuel|)]
    (fiber/set-fuel f budget)
    (fiber/resume f)
    (fiber/status f)))

(defn abandon-at (budget)
  (let [base (list 100 200)]
    (paused base budget)
    (sum base)))

(defn drive-keep (xs)
  (keep-if xs (list 100 200)))

(defn drive-native (c)
  (native-tail 1 (list 2 3) c))

(defn drive-later (c)
  (later c (list 1 2 3)))

(var i 0)
(while (%lt i 500)
  (assert (= (drive-keep (list 1 2 3)) 306)
          "u1: the kept elements and the accumulator are read whole")
  (assert (= (drive-keep (list -1 -2)) 300)
          "u2: the moved accumulator is read whole")
  (assert (= (drive-keep (list 1 -2 3)) 304) "u1 and u2: both arms on one walk")
  (assert (= (drive-native true) 3) "u3: the native callee reads the pair")
  (assert (= (drive-native false) 2)
          "u3: the native callee reads the accumulator")
  (assert (= (drive-later (list 1)) 12)
          "u4: the later operand reads the accumulator")
  (assert (= (drive-later (list -1)) 9) "u4: the accumulator arm moves it")
  (assert (= (keep-bound (list 1 -2 3) (list 100 200)) 304)
          "u5: the moved pair and the accumulator are read whole")
  (assert (= (tail-of-bound (list 1 -2) (list 100 200)) 30)
          "u6: the tail of the bound list is read whole")
  (assert (= (tail-of-bound (list -2 1) (list 100 200)) 31)
          "u6: the bound list one arm moves is read whole")
  (assert (= (native-bound 1 (list 2 3) true) 3)
          "u7: the native callee reads the bound pair")
  (assert (= (native-bound 1 (list 2 3) false) 2)
          "u7: the native callee reads the accumulator")
  (assert (= (keep-or (list 1 -2 3) (list 100 200)) 304)
          "u8: the kept elements and the accumulator are read whole")
  (assert (= (or-if (list 1 2) (list 100 200)) 300)
          "u9: the accumulator the second operand hands over is read whole")
  (assert (= (or-if (list -1 2) (list 100 200)) 299)
          "u9: both operands on one walk")
  (assert (= (later-or (list 1) (list 1 2 3)) 12)
          "u10: the later operand reads the accumulator")
  (assert (= (later-or (list -1) (list 1 2 3)) 9)
          "u10: the second operand moves the accumulator")
  (assert (= (keep-later (list 1 2 3) (list 100 200) 0) 609)
          "d1: the later operand reads the accumulator whole")
  (assert (= (keep-later (list 1 -2 3) (list 100 200) 0) 605)
          "d1: both arms on one walk")
  (assert (= (keep-callee (list 1 -2 3) (list 100 200)) 304)
          "d2: the callee reads the accumulator whole")
  (let [kept (native-later 1 (list 2 3) true)]
    (assert (= (sum (first kept)) 6) "d3: the native callee reads the pair")
    (assert (= (first (rest kept)) 5) "d3: the later operand reads acc"))
  (assert (= (native-later 1 (list 2 3) false) (list (list 2 3) 5))
          "d3: the native callee reads the accumulator")
  (assign i (%add i 1)))

# The walk spends 89 units of fuel, so these budgets pause it at each charge.
(var budget 1)
(while (%lt budget 92)
  (assert (= (abandon-at budget) 300)
          "d4: the driver reads the list an abandoned walk was handed")
  (assign budget (%add budget 1)))

(println "region-tail-operand-arm-uaf: ok")
