(elle/epoch 15)
# audited: 2026-10-20
# A container owns a view as a member and releases the source once, and never frees a source under an escaping view.
# docs/impl/region/effects.md
#
# The sidecar arms guardfree, so a read of a freed payload faults.
#
# A view is a header over its source's pages, and the walk records the embed
# edge result ⊇ source for the ownership forest. The forest counts the edge
# against external uniqueness and builds no subtree from it. A container
# therefore owns a view as an ordinary member, and the source stays Shared,
# released by the view's recorded edge at the container's drop. A container
# that owns the source of an escaping view never adopts the source: the edge
# leaves the container's subtree.
#
# The counter-factual, by shape. With the embed edge dropped from the
# uniqueness scan, the escaping shapes adopt the source into the container and
# free it under the view, a fault at the read. With the source released twice
# at the container's drop, the owning shapes fault at the read after the
# container dies.

# A container owns the string view; the source outlives the container.
(let [s (string/repeat "ab" 2048)]
  (let [c (@array)
        v (slice s 2 6)]
    (push c v)
    (assert (= (get c 0) "abab") "the string view reads from its owner"))
  (assert (= (slice s 0 4) "abab")
          "the string source reads after the container that owned its view died"))

# A container owns the bytes view; the source outlives the container.
(let [b (bytes (string/repeat "z" 4096))]
  (let [c (@array)
        v (slice b 2 6)]
    (push c v)
    (assert (= (get c 0) (bytes 122 122 122 122))
            "the bytes view reads from its owner"))
  (assert (= (slice b 0 2) (bytes 122 122))
          "the bytes source reads after the container that owned its view died"))

# A container owns the string source; the view escapes the container's scope.
(defn string-view-of-owned [n]
  (let [c (@array)
        s (string/repeat "ab" n)
        v (slice s 2 6)]
    (push c s)
    v))

(assert (= (string-view-of-owned 2048) "abab")
        "a string view outlives the container that owned its source")

# A container owns the bytes source; the view escapes the container's scope.
(defn bytes-view-of-owned [n]
  (let [c (@array)
        b (bytes (string/repeat "z" n))
        v (slice b 2 6)]
    (push c b)
    v))

(assert (= (bytes-view-of-owned 4096) (bytes 122 122 122 122))
        "a bytes view outlives the container that owned its source")

# A container owns the source and a string over it; the string escapes.
(defn string-over-owned [n]
  (let [c (@array)
        b (bytes (string/repeat "q" n))
        t (string (slice b 0 3))]
    (push c b)
    t))

(assert (= (string-over-owned 4096) "qqq")
        "a string over owned bytes outlives the container")

(println "region-view-owner-uaf: ok")
