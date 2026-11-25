(elle/epoch 18)
# audited: 2026-11-24
# A value a loop makes and a break carries out survives both its releases wherever it goes next.
# docs/impl/region/breakloop.md
#
# The pass release frees each pass's value on the passes that loop on, and the
# exit release frees the carried value past the block. Each row hands a value
# somewhere one of those releases could reach too early: back to the caller,
# to a name read after the block, into a container outside the loop, or into
# a closure. The sidecar arms guardfree, so a release that frees a live value
# faults at the read.

(defn returned [m]
  (var k 0)
  (forever
    (let [b (string/repeat "x" (inc k))]
      (when (>= k m) (break b)))
    (assign k (inc k))))

(defn read-after [m]
  (var k 0)
  (let [r (forever
            (let [b (string/repeat "y" (inc k))]
              (when (>= k m) (break b)))
            (assign k (inc k)))]
    (string r "!")))

(defn kept-each-pass [m]
  (def acc @[])
  (var k 0)
  (let [r (forever
            (let [b (string/repeat "z" (inc k))]
              (push acc b)
              (when (>= k m) (break b)))
            (assign k (inc k)))]
    [r (freeze acc)]))

(defn captured-each-pass [m]
  (def acc @[])
  (var k 0)
  (forever
    (let [b (string/repeat "w" (inc k))]
      (push acc (fn [] b))
      (when (>= k m) (break b)))
    (assign k (inc k)))
  (map (fn [f] (f)) acc))

(defn stored-after [m outer]
  (var k 0)
  (let [r (forever
            (let [b @[(inc k)]]
              (when (>= k m) (break b)))
            (assign k (inc k)))]
    (push outer r)
    outer))

(def @returned-call returned)
(def @read-after-call read-after)
(def @kept-call kept-each-pass)
(def @captured-call captured-each-pass)
(def @stored-call stored-after)

(def @i 0)
(while (< i 200)
  (assert (= (returned-call 4) "xxxxx") "the carried value returns")
  (assert (= (read-after-call 3) "yyyy!")
          "the carried value reads after the block")
  (let [pair (kept-call 3)]
    (assert (= (get pair 0) "zzzz") "the carried value beside the kept ones")
    (assert (= (get pair 1) ["z" "zz" "zzz" "zzzz"])
            "every pass's value survives in the container"))
  (assert (= (captured-call 2) ["w" "ww" "www"])
          "every pass's captured value survives")
  (let [outer (stored-call 3 @[])]
    (assert (= (get (get outer 0) 0) 4)
            "the carried value stored after the block"))
  (assign i (inc i)))

(println "region-break-loop-carry-uaf: ok")
