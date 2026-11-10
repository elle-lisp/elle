(elle/epoch 15)
# audited: 2026-11-10
# A release moved ahead of a frame-replacing tail call frees nothing the arm's value still reaches.
# docs/impl/region/operand.md
#
# The soundness face of region-tail-operand-let.lisp. The operand reading
# exempts only the regions of the value an operand produces, so the release of
# a value a `let` binds moves ahead of the call whenever the arms hand the
# callee something else. Each witness below hands the callee a value that
# still reaches the bound one, and the callee reads through it after the moved
# release has run. Under `--trace=guardfree` a freed page faults at the read.
#
# (u1) the arm's value embeds the bound array by a count of its own: a list
#      over it, whose alloc-time scan counts the embedding.
# (u2) the arm's value is an element borrowed out of the bound array through
#      `get`, whose pass-through retain is the count the callee reads under.
# (u3) the arm's value is the bound value itself: the ownership move, whose
#      release stays behind the call for the callee's own release to consume.

(def flag (> (length (sys/argv)) 100))

(defn sink-first (h)
  (block :keep
    (length (first h))))

(defn sink-length (h)
  (block :keep
    (length h)))

(defn embeds (x c)
  (sink-first (let [a (array x x x)]
                (if c (list a) (list a 0)))))

(defn borrows (x c)
  (sink-length (let [a (array (array x) (array x x))]
                 (if c (get a 0) (get a 1)))))

(defn moves (x c)
  (sink-length (let [a (array x x)]
                 (if c a a))))

(var i 0)
(while (%lt i 500)
  (assert (= (embeds 1 flag) 3) "u1: the embedded array is read whole")
  (assert (= (embeds 1 (not flag)) 3) "u1: the then arm embeds it too")
  (assert (= (borrows 1 flag) 2) "u2: the borrowed element is read whole")
  (assert (= (borrows 1 (not flag)) 1) "u2: the then arm borrows the other")
  (assert (= (moves 1 flag) 2) "u3: the moved array is read whole")
  (assign i (%add i 1)))

(println "region-tail-operand-let-uaf: ok")
