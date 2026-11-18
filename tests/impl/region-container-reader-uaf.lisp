(elle/epoch 15)
# audited: 2026-11-17
# A name bound to a loop's var keeps the value it read after the loop's next assign of that var.
# docs/impl/region/reads.md
#
# The sidecar arms guardfree, so a read of a freed value faults where it reads.
#
# Each shape binds a name to the whole value a `while` loop's var holds, then
# assigns the var, then reads the name. Functionalization makes the var a
# parameter of the loop, and the assign a slot write of that parameter. The
# write drops what the parameter held, so the name needs a counted reference of
# its own.
#
# The counter-factual: the parameter read immutable, so the read took no count.
# On the second pass the name held the array the first pass stored, which the
# assign had just freed. A var that starts as nil failed the same way even with
# the parameter counted, because the walk saw no region at the read and skipped
# the count.

# (a) a def reader of the loop's var: the shape of issue 1484.
(defn a-run [n]
  (def out @[])
  (var c (array 0))
  (var i 0)
  (while (< i n)
    (def t c)
    (assign c (array i))
    (push out (get t 0))
    (assign i (+ i 1)))
  (freeze out))

# (b) a let reader of the loop's var.
(defn b-run [n]
  (def out @[])
  (var c (array 0))
  (var i 0)
  (while (< i n)
    (let [t c]
      (assign c (array i))
      (push out (get t 0)))
    (assign i (+ i 1)))
  (freeze out))

# (c) the var starts as nil, so the walk sees no region at the read.
(defn c-run [n]
  (def out @[])
  (var c nil)
  (var i 0)
  (while (< i n)
    (def t c)
    (assign c (array i))
    (push out (if (nil? t) -1 (get t 0)))
    (assign i (+ i 1)))
  (freeze out))

# (d) the same, with the vars bound by a let.
(defn d-run [n]
  (def out @[])
  (let [@c nil
        @i 0]
    (while (< i n)
      (def t c)
      (assign c (array i))
      (push out (if (nil? t) -1 (get t 0)))
      (assign i (+ i 1))))
  (freeze out))

# (e) the control: `each` does not carry the var as a loop parameter.
(defn e-run []
  (def out @[])
  (var c (array 0))
  (each i in [0 1 2 3]
    (def t c)
    (assign c (array i))
    (push out (get t 0)))
  (freeze out))

# (f) 300 passes over arrays that hold a string and an array, so a plain run
# without guardfree reuses the freed memory and reads the wrong values.
(defn f-run [n]
  (def out @[])
  (var c (array "s-0" @[0]))
  (var i 0)
  (while (< i n)
    (def t c)
    (assign c (array (string "s-" (+ i 1)) @[(+ i 1)]))
    (push out [(get t 0) (get (get t 1) 0)])
    (assign i (+ i 1)))
  (freeze out))

(assert (= (a-run 4) [0 0 1 2])
        "a def reader keeps the value the pass before stored")
(assert (= (b-run 4) [0 0 1 2])
        "a let reader keeps the value the pass before stored")
(assert (= (c-run 4) [-1 0 1 2]) "a reader of a var that starts nil is counted")
(assert (= (d-run 4) [-1 0 1 2])
        "a reader of a let-bound var that starts nil is counted")
(assert (= (e-run) [0 0 1 2]) "the each control reads its var as before")

(def f-out (f-run 300))
(var f-bad 0)
(var f-k 0)
(while (< f-k 300)
  (unless (= (get f-out f-k) [(string "s-" f-k) f-k]) (assign f-bad (+ f-bad 1)))
  (assign f-k (+ f-k 1)))
(assert (= f-bad 0) "300 passes each read the value the pass before stored")

(println "region-container-reader-uaf: ok")
