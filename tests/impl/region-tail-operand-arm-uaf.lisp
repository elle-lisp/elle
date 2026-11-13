(elle/epoch 15)
# audited: 2026-11-12
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
  (assign i (%add i 1)))

(println "region-tail-operand-arm-uaf: ok")
