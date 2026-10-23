(elle/epoch 15)
# audited: 2026-10-23
# An @struct that grows claims region pages, and one written within its capacity claims none.
# docs/impl/mutable.md
#
# An @struct keeps its entries in a tree whose nodes lie in one extent of its
# container's region, so a growth claims room there and `arena/page-claims`
# counts it. The controls rebind every key, and delete the last key of the
# grown store and put it back, which fits its leaf and claims nothing.
#
# The counter-factual: a store on the Rust heap grows by allocating B-tree
# nodes, which claims no page, so the growth reads zero claims, as the controls
# do.
#
# The traps. A compile claims pages for its working region, and so does a
# promotion landing between two readings. The sidecar compiles each function on
# its first call, on the VM thread, and the warm-up below makes that call before
# any reading. A loop counter steps with `%add`, because `+` collects its
# arguments into a rest list that claims a page per call. The keys and values
# are integers, so no entry claims a page of its own and no key is interned.
# They rise, so each put lands in the last leaf, and the churn touches the last
# key of that leaf. A delete there at most borrows from the leaf's sibling, and
# the put back fits the leaf, so no write needs a node.

(def size 65536)

(defn grow [store n]
  "Put each integer below N into STORE under itself."
  (var i 0)
  (while (< i n)
    (put store i i)
    (assign i (%add i 1))))

(defn churn [store key n]
  "Delete KEY from STORE and put it back, N times."
  (var i 0)
  (while (< i n)
    (del store key)
    (put store key i)
    (assign i (%add i 1))))

(grow @{} 2)
(churn @{1 1} 1 2)

(def s @{})
(def s-grew
  (let [p0 (arena/page-claims)]
    (grow s size)
    (%sub (arena/page-claims) p0)))
(assert (= (length s) size) "every put into the @struct lands")
(assert (> s-grew 0) "an @struct that grows claims region pages")

(def s-rebound
  (let [p0 (arena/page-claims)]
    (grow s size)
    (%sub (arena/page-claims) p0)))
(assert (= s-rebound 0) "an @struct whose every key is rebound claims no page")
(assert (= (length s) size) "a rebind adds no entry")

(def top (%sub size 1))
(def s-churned
  (let [p0 (arena/page-claims)]
    (churn s top size)
    (%sub (arena/page-claims) p0)))
(assert (= s-churned 0) "an @struct written within its capacity claims no page")
(assert (= (length s) size) "the churn leaves every entry in place")
(assert (= (get s top) top) "and the churned key holds its last value")

(println "mutable-struct-pages: ok")
