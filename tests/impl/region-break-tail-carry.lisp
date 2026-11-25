(elle/epoch 18)
# audited: 2026-11-24
# A tail call a `break` carries takes a replica of each release on the break's path.
# docs/impl/region/breakpoint.md
#
# A `break` out of a function's tail block walks its value as a tail value, so a
# closure call there replaces the frame before the jump. Each release the
# break's path owes after that call, past the block's exit label or jumped over
# inside the block, then runs on the closure path only as a replica ahead of the
# call. Without it the region strands once per call.
#
# This file is the LEAK gauge: an `arena/region-count` delta over a fixed
# window, which must be BOUNDED for each face. The soundness complement is
# region-break-tail-carry-uaf.lisp.

(def window 2000)

(include-file "gauge/window.lisp")

(defn mk [k]
  (bytes (string/repeat "x" (inc k))))

# subjects ─────────────────────────────────────────────────────────────────────

# (a) the break carries `(apply concat chunks)`. `concat` is a closure, and
# `chunks` is released past the block's exit label. Every value pushed into it
# strands behind it, so the counter-factual reads m+1 regions per call.
(defn concat-chunks [m]
  (let [chunks @[]]
    (var k 0)
    (forever
      (when (>= k m) (break (apply concat chunks)))
      (push chunks (mk k))
      (assign k (inc k)))))

# (b) the call sits in an arm of the value the break carries, as `tls/read-all`
# writes it. The arm's point covers the break.
(defn concat-in-arm [m]
  (let [chunks @[]]
    (var k 0)
    (forever
      (when (>= k m)
        (break (if (> (length chunks) 0) (apply concat (freeze chunks)) (bytes))))
      (push chunks (mk k))
      (assign k (inc k)))))

# (c) a value bound outside the loop and read inside it, with a closure callee
# passed in. Its release lands past the exit label.
(defn outer [g m]
  (let [held (mk 3)]
    (var k 0)
    (forever
      (when (>= k m) (break (g k k)))
      (assign k (+ k (length held))))))

# (d) a value made on each pass and released after the branch, a position the
# break jumps over. The breaking pass's value strands without the replica.
(defn jumped [g m]
  (var k 0)
  (forever
    (let [x (mk k)]
      (when (>= k m) (break (g k k)))
      (assign k (+ k (length x))))))

# controls ─────────────────────────────────────────────────────────────────────
# Each reads 0 already: a native in the carried position returns to the frame
# and passes the exit label, the `while` makes its tail call after the block,
# and a break carrying `chunks` itself hands it on.
(defn ctl-native [m]
  (let [chunks @[]]
    (var k 0)
    (forever
      (when (>= k m) (break (freeze chunks)))
      (push chunks (mk k))
      (assign k (inc k)))))

(defn ctl-while [m]
  (let [chunks @[]]
    (var k 0)
    (while (< k m)
      (push chunks (mk k))
      (assign k (inc k)))
    (apply concat chunks)))

(defn ctl-carry-chunks [m]
  (let [chunks @[]]
    (var k 0)
    (forever
      (when (>= k m) (break chunks))
      (push chunks (mk k))
      (assign k (inc k)))))

# The callee reaches `outer` and `jumped` through a parameter, so no graft
# inlines it and the call stays a frame-replacing one.
(def @pick (fn [a b] a))

(def concat-chunks-d (measure-regions (fn [] (concat-chunks 5)) 200 window))
(def concat-in-arm-d (measure-regions (fn [] (concat-in-arm 5)) 200 window))
(def outer-d (measure-regions (fn [] (outer pick 3)) 200 window))
(def jumped-d (measure-regions (fn [] (jumped pick 3)) 200 window))
(def ctl-native-d (measure-regions (fn [] (ctl-native 5)) 200 window))
(def ctl-while-d (measure-regions (fn [] (ctl-while 5)) 200 window))
(def ctl-carry-chunks-d
  (measure-regions (fn [] (ctl-carry-chunks 5)) 200 window))

(println "region-break-tail-carry deltas over " window " iters:")
(println "  concat-chunks " concat-chunks-d "  concat-in-arm " concat-in-arm-d)
(println "  outer " outer-d "  jumped " jumped-d)
(println "  controls: native " ctl-native-d "  while " ctl-while-d
         "  carry-chunks " ctl-carry-chunks-d)

# Every leak in this class is at least one whole region per call, so a surviving
# over-keep reads ~2000 over the window. 100 is slack for the one-time intercept.
(def bounded? (bounded-by 100))

(bounded? ctl-native-d "control: a native the break carries")
(bounded? ctl-while-d "control: the tail call after a `while`")
(bounded? ctl-carry-chunks-d "control: the break carries `chunks` itself")

(bounded? concat-chunks-d "a release past the exit label, behind `concat`")
(bounded? concat-in-arm-d "the carried call in an arm of the break's value")
(bounded? outer-d "a value bound outside the loop, behind a parameter callee")
(bounded? jumped-d "a release the break jumps over, behind a parameter callee")

# Value preservation: a moved release must not change what runs.
(assert (= (concat-chunks 2) (bytes "xxx"))
        "concat-chunks joined the wrong chunks")
(assert (= (concat-in-arm 2) (bytes "xxx"))
        "concat-in-arm joined the wrong chunks")
(assert (= (concat-in-arm 0) (bytes)) "concat-in-arm lost its empty arm")
(assert (= (outer pick 3) 4) "outer stopped on the wrong pass")
(assert (= (jumped pick 3) 3) "jumped stopped on the wrong pass")

(println "region-break-tail-carry: ok")
