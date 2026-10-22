(elle/epoch 15)
# audited: 2026-10-22
# An @array that grows claims region pages, and one written within its capacity claims none.
# docs/impl/mutable.md
#
# An @array keeps its elements in an extent of its container's region, so a
# growth claims room there and `arena/page-claims` counts it. The control
# overwrites the first element of the grown store in place, which fits its
# capacity and claims nothing.
#
# The counter-factual: a store on the Rust heap grows by `realloc`, which
# claims no page, so the growth reads zero claims, as the control does.
#
# The traps. A compile claims pages for its working region, and so does a
# promotion landing between two readings. The sidecar compiles each function on
# its first call, on the VM thread, and the warm-up below makes that call before
# any reading. A loop counter steps with `%add`, because `+` collects its
# arguments into a rest list that claims a page per call. The elements are
# integers, so no element claims a page of its own.

(def size 65536)

(defn grow [store piece n]
  "Push PIECE onto STORE N times."
  (var i 0)
  (while (< i n)
    (push store piece)
    (assign i (%add i 1))))

(defn rewrite [store piece n]
  "Put PIECE over the first element of STORE, N times."
  (var i 0)
  (while (< i n)
    (put store 0 piece)
    (assign i (%add i 1))))

(grow @[] 7 2)
(rewrite @[1] 7 2)

(def a @[])
(def a-grew
  (let [p0 (arena/page-claims)]
    (grow a 7 size)
    (%sub (arena/page-claims) p0)))
(assert (= (length a) size) "every push onto the @array lands")
(assert (> a-grew 0) "an @array that grows claims region pages")

(def a-rewrote
  (let [p0 (arena/page-claims)]
    (rewrite a 9 size)
    (%sub (arena/page-claims) p0)))
(assert (= a-rewrote 0) "an @array written within its capacity claims no page")
(assert (= (get a 0) 9) "the write lands")
(assert (= (get a 1) 7) "and moves nothing else")

(println "mutable-array-pages: ok")
