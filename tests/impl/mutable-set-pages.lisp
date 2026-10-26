(elle/epoch 15)
# audited: 2026-10-23
# An @set that grows claims region pages, and one written within its capacity claims none.
# docs/impl/mutable.md
#
# An @set keeps its members in a tree whose nodes lie in one extent of its
# container's region, so a growth claims room there and `arena/page-claims`
# counts it. The control deletes the last member of the grown store and adds it
# back, which fits its leaf and claims nothing.
#
# The counter-factual: a store on the Rust heap grows by allocating B-tree
# nodes, which claims no page, so the growth reads zero claims, as the control
# does.
#
# The traps. A compile claims pages for its working region, and so does a
# promotion landing between two readings. The sidecar compiles each function on
# its first call, on the VM thread, and the warm-up below makes that call before
# any reading. A loop counter steps with `%add`, because `+` collects its
# arguments into a rest list that claims a page per call. The members are
# integers, so no member claims a page of its own. They rise, so each add lands
# in the last leaf, and the churn touches the last member of that leaf. A
# delete there at most borrows from the leaf's sibling, and the add back fits
# the leaf, so no write needs a node.

(def size 65536)

(defn grow [store n]
  "Add the integers below N to STORE."
  (var i 0)
  (while (< i n)
    (add store i)
    (assign i (%add i 1))))

(defn churn [store member n]
  "Delete MEMBER from STORE and add it back, N times."
  (var i 0)
  (while (< i n)
    (del store member)
    (add store member)
    (assign i (%add i 1))))

(grow @|| 2)
(churn @|1| 1 2)

(def s @||)
(def s-grew
  (let [p0 (arena/page-claims)]
    (grow s size)
    (%sub (arena/page-claims) p0)))
(assert (= (length s) size) "every add to the @set lands")
(assert (> s-grew 0) "an @set that grows claims region pages")

(def top (%sub size 1))
(def s-churned
  (let [p0 (arena/page-claims)]
    (churn s top size)
    (%sub (arena/page-claims) p0)))
(assert (= s-churned 0) "an @set written within its capacity claims no page")
(assert (= (length s) size) "the churn leaves every member in place")
(assert (contains? s top) "and the churned member is back")

(println "mutable-set-pages: ok")
