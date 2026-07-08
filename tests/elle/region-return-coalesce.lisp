(elle/epoch 12)
# region-return-coalesce.lisp — transform 1, the coalesced return mint
# (docs/impl/region/mechanism.md § "Compile-time region selection (coalescing)").
#
# When a function returns a value that is a fresh local allocation whose region is
# a known static slot, `lower_return` mints slot-resolved (`IncrefRegion`, guarded
# under debug by the `AssertRegionMatches` equivalence oracle) instead of
# value-resolved (`IncrefValueRegion`). The slot resolves — through the activation
# map — to the same physical region the value lives in, so the substitution is
# RC-neutral: it must be a no-op at runtime, in particular introducing no
# use-after-free.
#
# This pins the coalesced path's CORRECTNESS end-to-end. A heap literal returned
# directly from a thunk (`(fn [] "lit")`, `(fn [] '(1 2 3))`) is the canonical
# fresh-aggregate return: the literal materializes fresh each call
# (docs/impl/region/model.md § "Constants lower as ordinary allocations"), in its
# own region, and the return is the coalesced site. A mis-coalesce — the slot
# resolving to a wrong/dead physical region — frees the returned value's region
# early; the junk allocation between the return and the read reuses that region, so
# the wrong-value asserts catch the stale read deterministically (the
# region-mutable-reassign-flow manifestation trick), `--trace=guardfree` detonates
# on the freed page, and the debug `AssertRegionMatches` oracle panics at the exact
# coalesced instruction.
#
# Scope: this is a no-UAF pin. "No NEW leak" is RC-neutrality, which the residue
# oracle owns (tests/region_process_teardown.rs — the count is unchanged by the
# value→slot substitution). It is deliberately NOT pinned here: a fresh heap
# literal returned and dropped carries the pre-existing discarded-result /
# native-result leak (region-native-result-leak.lisp, and the `discard-*` /
# `native-tail-*` probes in oracle.lisp), so a boundedness assert would re-pin a
# known leak class, not C2.

(defn ret-string []
  "coalesced")

(defn ret-list []
  '(1 2 3))

# Survival: build a coalesced return, allocate junk, then read it back. An
# early-freed (mis-coalesced) region would have been reused by the junk, corrupting
# these reads.
(var i 0)
(while (%lt i 200)
  (let [s (ret-string)
        l (ret-list)
        _junk (ret-string)]
    (assert (= s "coalesced") (string "coalesced string corrupted at i=" i))
    (assert (= (length l) 3) (string "coalesced list corrupted at i=" i))
    (assert (= (get l 0) 1) (string "coalesced list head corrupted at i=" i)))
  (assign i (%add i 1)))

(println "region-return-coalesce: ok")
