(elle/epoch 12)
# tests/integration/fixtures/region-traits-table-uaf.lisp
#
# Quarantined here — NOT under tests/elle/ — because BEFORE the fix it SIGSEGV'd
# on plain runs (5/5), and `make smoke` globs tests/elle/*.lisp into one shared
# process where a segfault would take the whole harness down. It is exercised by
# the guardfree subprocess pin in tests/integration/elle_scripts.rs
# (`region_traits_table_uaf`); it now exits 0 (fixed), and faults deterministically
# under guardfree if the trait-table cross-region edge is ever dropped again.
#
# WHAT IT REPRODUCES
#   The elle corpus crashes in `tests/elle/sorted-struct.lisp` and
#   `tests/elle/traits.lisp`. Both reduce (independent delta-debug runs) to the
#   same two-form shape below: attach a trait table to a value with `with-traits`,
#   then read a key out of that table via `(get (traits x) key)`. The crash is a
#   binary-search comparison over freed pages:
#
#     #1 TableKey::cmp                src/value/types.rs:288   <- self in freed pages
#     #2 sorted_struct_get::{closure} src/value/types.rs:382
#     #3 core::slice::binary_search_by
#     #4 sorted_struct_get            src/value/types.rs:382
#     #5 prim_get                     src/primitives/access.rs
#
# ROOT CAUSE (--trace=guardfree, not a guess)
#   guardfree attributes the fault precisely:
#     use-after-free … freed by region R via direct
#     free site: DecrefValueRegion of struct (runtime region R) @ this file, the
#                `with-traits` form
#   `clone_with_traits` (src/primitives/traits.rs:74) stores the trait-table
#   struct into the new object's `traits` side-field WITHOUT increfing the table's
#   region. The alloc-time content scan (Rule 5, "immutable contents — alloc_obj
#   scans the new object and increfs each region its fields point into") does not
#   cover the `traits` side-field, so the table region keeps RC 1 (its initial
#   reference). At the `with-traits` call's decref_point the solver-emitted
#   DecrefValueRegion drops it to 0 and frees it — a DIRECT free of a still-live
#   value (a liveness bug: the new value's `traits` field still references it).
#   `(traits t)` then hands back the dangling table and `(get … :tag)` binary-
#   searches its freed pages. Missing the attach-time incref is the escape-site
#   gap; its symmetric partner is the free-cascade decref of `traits` (Rule 7).
#
# THE FIX: `find_object_cross_refs` (src/value/fiberheap/regionpool/introspect.rs)
# now enumerates `obj.traits()` for every variant, so the alloc-scan increfs the
# table's region and the free-cascade decrefs it symmetrically (Rule 5/7) — the
# table outlives its constructor's DecrefRegion and dies with its host.

(def t (with-traits @[1 2 3] {:tag :x}))
(assert (= (get (traits t) :tag) :x)
        "trait table survives its constructor's decref")
(println "ok")
