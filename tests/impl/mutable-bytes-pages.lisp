(elle/epoch 15)
# audited: 2026-10-22
# An @bytes or @string that grows claims region pages, and one written within its capacity claims none.
# docs/impl/mutable.md
#
# A byte store keeps its bytes in an extent of its container's region, so a
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
# arguments into a rest list that claims a page per call. And the `@string`
# control writes 64 times, not `size`, because a `put` on an `@string` walks
# every grapheme to find its index. The piece the control writes is bound before
# the reading, because a literal materializes into a region of its own.

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

(grow (@bytes) 7 2)
(rewrite @b[1] 7 2)
(grow @"" "x" 2)
(rewrite @"a" "x" 2)

# ── @bytes ────────────────────────────────────────────────────────────────────

(def b (@bytes))
(def b-grew
  (let [p0 (arena/page-claims)]
    (grow b 7 size)
    (%sub (arena/page-claims) p0)))
(assert (= (length b) size) "every push onto the @bytes lands")
(assert (> b-grew 0) "an @bytes that grows claims region pages")

(def b-rewrote
  (let [p0 (arena/page-claims)]
    (rewrite b 9 size)
    (%sub (arena/page-claims) p0)))
(assert (= b-rewrote 0) "an @bytes written within its capacity claims no page")
(assert (= (get b 0) 9) "the write lands")
(assert (= (get b 1) 7) "and moves nothing else")

# ── @string ───────────────────────────────────────────────────────────────────

(def s @"")
(def s-grew
  (let [p0 (arena/page-claims)]
    (grow s "x" size)
    (%sub (arena/page-claims) p0)))
(assert (= (length s) size) "every push onto the @string lands")
(assert (> s-grew 0) "an @string that grows claims region pages")

(def y "y")
(def s-rewrote
  (let [p0 (arena/page-claims)]
    (rewrite s y 64)
    (%sub (arena/page-claims) p0)))
(assert (= s-rewrote 0) "an @string written within its capacity claims no page")
(assert (= (string (slice s 0 2)) "yx")
        "the write lands, and moves nothing else")

(println "mutable-bytes-pages: ok")
