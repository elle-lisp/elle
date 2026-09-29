(elle/epoch 13)
# audited: 2026-09-29
# A trait table attached with with-traits is counted by its host, so reading it back through traits finds it live.
# docs/impl/region/rules.md
#
# The sidecar arms guardfree.
#
# THE RULE. `find_object_cross_refs` (src/value/fiberheap/regionpool/introspect.rs)
# enumerates `obj.traits()` for every variant. So the alloc-time content scan
# (Rule 5, immutable contents) increfs the trait table's region when
# `clone_with_traits` stores it into the new object's `traits` side-field, and
# the free cascade decrefs it symmetrically (Rule 7). The table outlives its
# constructor's `DecrefRegion` and dies with its host.
#
# THE COUNTER-FACTUAL. A scan that skips the side-field leaves the table at RC 1,
# so the `DecrefValueRegion` at the `with-traits` call's decref_point frees it
# while the new value's `traits` field still names it — a DIRECT free of a live
# value. `(traits t)` then hands back the dangling table, and `(get … :tag)`
# binary-searches its freed pages: a plain run faults in `TableKey::cmp` inside
# `sorted_struct_get` (src/value/types/sorted.rs). tests/lang/sorted-struct.lisp
# and tests/lang/traits.lisp reach the same two-form shape.

(def t (with-traits @[1 2 3] {:tag :x}))
(assert (= (get (traits t) :tag) :x)
        "trait table survives its constructor's decref")
(println "ok")
