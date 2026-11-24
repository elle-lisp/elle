(elle/epoch 18)
# audited: 2026-11-24
# A value a branch's test makes or reads survives its release wherever a name or a container takes it.
# docs/impl/region/lastuse.md
#
# A branch's test is its own last use, so its release runs before the branch
# chooses an arm. Each row hands the test's value somewhere that release could
# reach too early: to a name the arm reads, into a container read after the
# loop, or into a closure. The sidecar arms guardfree, so a release that frees
# a live value faults at the read.

(defn arm-reads-name [n]
  (let [s (string "a" n)]
    (if s (string s "!") "")))

(defn and-test-arm-reads [n]
  (let [s (string "b" n)]
    (if (and (> n 0) s) (string s "?") s)))

(defn cond-test-arm-reads [n]
  (let [s (string "c" n)]
    (cond
      (and (> n 5) s) (string s "<")
      s (string s ">")
      "")))

(defn kept-from-test [n]
  (def acc @[])
  (var k 0)
  (while (and (< k n)
              (let [b (string "d" k)]
                (push acc b)
                b))
    (assign k (inc k)))
  (freeze acc))

(defn captured-in-test [n]
  (def acc @[])
  (var k 0)
  (while (and (< k n)
              (let [b (string "e" k)]
                (push acc (fn [] b))
                b))
    (assign k (inc k)))
  (map (fn [f] (f)) acc))

(defn element-kept [gs x]
  (def acc @[])
  (var k 0)
  (while (let [e (gs k)]
           (when (= e x)
             (push acc e)
             true))
    (assign k (inc k)))
  (freeze acc))

(def @arm-call arm-reads-name)
(def @and-call and-test-arm-reads)
(def @cond-call cond-test-arm-reads)
(def @kept-call kept-from-test)
(def @captured-call captured-in-test)
(def @element-call element-kept)

(def @i 0)
(while (< i 200)
  (assert (= (arm-call 1) "a1!") "the arm reads the name its test read")
  (assert (= (and-call 2) "b2?") "the arm reads a name an and test handed on")
  (assert (= (cond-call 3) "c3>") "a later clause reads a name its test read")
  (assert (= (kept-call 3) ["d0" "d1" "d2"])
          "each value a test made survives in the container")
  (assert (= (captured-call 3) ["e0" "e1" "e2"])
          "each value a test made survives in a closure")
  (assert (= (element-call (->array "--x") "-") ["-" "-"])
          "each element a test read survives in the container")
  (assign i (inc i)))

(println "region-loop-test-release-uaf: ok")
