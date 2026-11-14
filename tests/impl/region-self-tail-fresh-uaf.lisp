(elle/epoch 15)
# audited: 2026-11-13
# A self tail call's trip releases nothing an earlier trip moved to the call, with every function compiled.
# docs/impl/region/rules.md
#
# The JIT runs a self tail call as a loop over one activation. Each witness
# writes a slot on one trip and hands its value to the self tail call. The next
# trip takes a path that does not write the slot, and releases what the slot
# holds. Under `--trace=guardfree` a freed page faults at the later read.
#
# (f1) a binder whose init is a branch, each arm allocating a list. The trip
#      that hands the call the accumulator releases both arms' lists ahead of
#      its `TailCall`.
# (f2) a value an earlier branch binds, handed to the call by one arm of a
#      branch operand. The sibling arm releases it at the arm's end.
#
# The counter-factual: the loop header jumped straight to the first block. The
# slot still held the list the previous trip moved to the call, which is now
# the accumulator, and the release freed it.

(defn sum (xs)
  (if (empty? xs)
    0
    (+ (first xs) (sum (rest xs)))))

(defn both-arms (c acc)
  (if (empty? c)
    (sum acc)
    (let [x (if (< 0 (first c)) (list (first c) 10) (list 0 0))]
      (if (= 0 (first x)) (both-arms (rest c) acc) (both-arms (rest c) x)))))

(defn earlier-branch (c acc)
  (if (empty? c)
    (sum acc)
    (let [x (if (< 0 (first c))
              (let [t (list (first c) 10)]
                t)
              nil)]
      (earlier-branch (rest c) (if (nil? x) acc x)))))

(var i 0)
(while (%lt i 200)
  (assert (= (both-arms (list 1 -2) (list 100 200)) 11)
          "f1: the list one trip moved to the call survives the next trip")
  (assert (= (both-arms (list 1 -2 3 -4) (list 100 200)) 13)
          "f1: a longer walk alternates the arms")
  (assert (= (earlier-branch (list 1 -2) (list 100 200)) 11)
          "f2: the bound value one trip moved to the call survives the next trip")
  (assert (= (earlier-branch (list 1 -2 3 -4) (list 100 200)) 13)
          "f2: a longer walk alternates the arms")
  (assign i (%add i 1)))

(println "region-self-tail-fresh-uaf: ok")
