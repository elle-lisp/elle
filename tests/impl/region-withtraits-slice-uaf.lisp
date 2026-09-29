(elle/epoch 13)
# audited: 2026-09-29
# A with-traits clone of a slice-backed value owns its payload, so it outlives its source.
# docs/impl/region/effects.md
#
# The sidecar arms guardfree, so a read of the freed payload faults.
#
# `with-traits` is declared Fresh: the whole result lives in the call's own
# result region. Its clone of a slice-backed immutable (string, array, bytes,
# set) must therefore COPY the payload into the clone's region. RegionSlice is
# Copy, and copying the (ptr, len) pair instead aliases backing pages in the
# SOURCE's region with no counted edge (docs/impl/region/model.md). The source
# literal dies at its ordinary decref point right after the with-traits call,
# and that frees the aliased payload under the live clone.
#
# The counter-factual: `clone_with_traits` without the payload copy. The spawn
# facets then fail on any build, because the send serializer reads the freed,
# recycled pages and chases self-referential garbage to a stack overflow. The
# in-thread facets read stale but intact pages, which only guardfree turns
# into a fault.

# array, read across a spawn boundary (the witnessed shape)
(let [v (with-traits [1 2 3] {:tag :my-type})]
  (assert (= (sys/join (sys/spawn-vm (fn [] (first v)))) 1)
          "traited array survives its source and a spawn round-trip"))

# string, read across a spawn boundary
(let [s (with-traits (string "abc" "def") {:tag :s})]
  (assert (= (sys/join (sys/spawn-vm (fn [] (length s)))) 6)
          "traited string survives its source and a spawn round-trip"))

# in-thread reads after the source's demise (guardfree facets):
# touch every element / the whole payload.
(let [v (with-traits [10 20 30] {:tag :t2})]
  (assert (= (+ (get v 0) (+ (get v 1) (get v 2))) 60)
          "traited array elements readable in-thread"))

(let [s (with-traits (string "xy" "z") {:tag :t3})]
  (assert (= (length s) 3) "traited string readable in-thread"))

# traits themselves still attached and readable
(let [v (with-traits [7 8] {:tag :keep})]
  (assert (= (get (traits v) :tag) :keep) "trait table preserved"))

(println "region-withtraits-slice-uaf: ok")
