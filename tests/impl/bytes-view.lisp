(elle/epoch 15)
# audited: 2026-10-20
# A slice of immutable bytes or a string, and a string over immutable bytes, claims no page for its payload.
# docs/impl/values.md
#
# `arena/bytes` sums the pages every live region owns and the pages the pool
# caches. A page that moves from the cache to a region leaves the sum where it
# was, so one copy of a 1 MiB payload can read as free when the cache holds a
# page of its class. Eight copies held at once cannot: the cache is bounded at
# 4 MiB by default, so at least four of them map fresh pages. Eight views claim
# at most eight base pages, for their headers.
#
# The counter-factual: a `slice` that copied read 16 MiB for the first shape
# here, a 2 MiB page per copy.

(def mib 1048576)

(defn held-bytes [make]
  "The bytes arena/bytes grows by while eight values make answers are held."
  (let [before (arena/bytes)
        held [(make 0) (make 1) (make 2) (make 3) (make 4) (make 5) (make 6)
              (make 7)]
        after (arena/bytes)]
    (assert (= 8 (length held)) "every value is held while the gauge is read")
    (- after before)))

(def big (bytes (string/repeat "x" mib)))
(def text (string/repeat "y" mib))

(let [grew (held-bytes (fn [i] (slice big i (- mib 8))))]
  (assert (< grew mib)
          (string "a slice of immutable bytes claims no page for its payload: grew "
                  grew)))
(println "  1. a slice of bytes shares its source")

(let [grew (held-bytes (fn [i] (slice text i (- mib 8))))]
  (assert (< grew mib)
          (string "a slice of a string claims no page for its payload: grew "
                  grew)))
(println "  2. a slice of a string shares its source")

(let [grew (held-bytes (fn [i] (string (slice big i))))]
  (assert (< grew mib)
          (string "a string over immutable bytes claims no page for its payload: grew "
                  grew)))
(println "  3. a string over bytes shares its source")

(let [grew (held-bytes (fn [i] (slice (slice big i) 8 (- mib 16))))]
  (assert (< grew mib)
          (string "a slice of a slice claims no page for its payload: grew "
                  grew)))
(println "  4. a slice of a slice shares the first source")

(println "bytes-view: ok")
