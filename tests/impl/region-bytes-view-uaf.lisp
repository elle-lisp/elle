(elle/epoch 15)
# audited: 2026-10-20
# A view over immutable bytes or a string keeps its source's pages alive, so it outlives its source.
# docs/impl/values.md
#
# The sidecar arms guardfree, so a read of a freed payload faults.
#
# A view's header is born in the call's own region and its RegionSlice points
# into the source's pages. The alloc-time scan records that edge
# (docs/impl/values.md). Without it the source's ordinary demise frees the
# payload under the live view.
#
# The counter-factual: with the string and bytes arms left out of the scan,
# every shape below read freed pages under --trace=guardfree. A slice that
# copies passes too, which is why tests/impl/bytes-view.lisp pins the
# sharing.

# A source that dies at its decref point right after the call.
(let [v (slice (string/repeat "ab" 2048) 2 6)]
  (assert (= v "abab") "a view of a call's result reads after the result dies"))

# A source born in a frame the view leaves.
(defn head [n]
  (let [s (string/repeat "xy" n)]
    (slice s 0 4)))

(assert (= (head 1000) "xyxy") "a string view outlives its frame")

(defn tail [n]
  (let [b (bytes (string/repeat "z" n))]
    (slice b (- n 3))))

(assert (= (tail 5000) (bytes 122 122 122)) "a bytes view outlives its frame")

(defn text [n]
  (let [b (bytes (string/repeat "q" n))]
    (string (slice b 0 3))))

(assert (= (text 5000) "qqq") "a string over a bytes view outlives its frame")

# A view of a view: the middle dies first, then the source.
(defn mid [n]
  (let [a (string/repeat "m" n)
        b (slice a 10)]
    (slice b 5 8)))

(assert (= (mid 4096) "mmm") "a view of a view holds the first source")

# Read across a spawn boundary, where the send serializer walks the payload.
(let [v (slice (string/repeat "ab" 2048) 2 6)]
  (assert (= (sys/join (sys/spawn-vm (fn [] (length v)))) 4)
          "a view survives its source and a spawn round-trip"))

# Many views over one source, read after every source has died.
(defn views [n]
  (let [s (string/repeat "v" (* n 16))]
    (map (fn [i] (slice s (* i 16) (+ (* i 16) 4))) (range n))))

(each v in (views 64)
  (assert (= v "vvvv") "each of many views reads its source"))

(println "region-bytes-view-uaf: ok")
