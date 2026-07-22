(elle/epoch 12)
# Soundness guard for the %pair containment accounting.
#
# A cons cell (`%pair`/`pair`) that stores a HEAP value keeps that value's region
# alive for the cons's lifetime. The runtime records this at construction
# (`handle_list` → `incref_cross_region`, EscapeSite::ImmutableContents) and
# releases it in the free-time cascade (`find_object_cross_refs`, the `Pair`
# arm) — the same alloc-scan/free-cascade contract arrays and closure envs use.
# So the cons's stored heap content must survive until the cons itself dies,
# through deep region-id churn that recycles freed ids onto live ones.
#
# This exercises the escaping/stored cons whose element is read back AFTER the
# construction site's own scope has ended and many intervening allocations have
# recycled region ids: if the element's region were under-increfed (freed while
# the cons still points at it), the read faults under `--trace=guardfree` (the
# freed page is recycled and the stale deref detonates the generation stamp);
# without guardfree a just-freed page still reads intact, so guardfree is the
# oracle. The controls (immediate car/cdr) never allocate a content region.

# A cons holding two fresh heap values, returned to the caller (escapes its
# builder's frame) and read back after churn.
(defn mk-pair [i]
  (%pair {:a i :b (string "v" i)} (string "tail" i)))

(defn churn [n]
  (def @acc ())
  (def @k 0)
  (while (< k n)
    # each iteration builds a fresh heap-content cons and conses it onto acc;
    # earlier conses' contents must stay live while later ones recycle ids.
    (assign acc (%pair (mk-pair k) acc))
    (assign k (+ k 1)))
  acc)

# Build a deep chain, then walk it reading every stored heap content back. A
# premature free of any element's region faults on the read here.
(def chain (churn 200))
(def @cursor chain)
(def @sum 0)
(while (not (empty? cursor))
  (let [p (first cursor)]
    # p = (%pair {:a k :b "vk"} "tailk"); read both heap contents back
    (let [h (first p)
          t (rest p)]
      (assign sum (+ sum (get h :a)))
      (assign sum (+ sum (length (get h :b))))
      (assign sum (+ sum (length t)))))
  (assign cursor (rest cursor)))
(assert (> sum 0) "pair heap contents survived the churn")

# The stdlib list-building path (map builds conses holding fresh heap results)
# under read-back pressure — the production shape of the same edge.
(def mapped (map (fn [x] {:sq (* x x)}) (list 1 2 3 4 5)))
(assert (= (get (first mapped) :sq) 1)
        "map result conses hold live struct contents")
(assert (= (get (first (rest mapped)) :sq) 4) "second map struct content live")
(println "region-pair-heap-content-uaf: ok")
