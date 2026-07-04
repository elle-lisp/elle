(elle/epoch 12)
# Counterfactual: calling a STRUCT as a function to look up a key — `(m :k)` —
# returns the co-located VALUE without a pass-through retain on the struct's
# region, so releasing the struct frees the value under its consumer's borrow:
# use-after-free.
#
# This is the STRUCT sibling of region-array-element-uaf.lisp. Both are the same
# root defect: the collection-as-function call-index path (`call_collection`,
# src/vm/call.rs) does NOT incref the returned element's region the way `get`/
# `first` DO (docs/impl/region-rules.md Rule 5, native-result pass-through). A fix for the
# array case must cover structs too — this file pins that obligation so the next
# agent's `call_collection` retain lands for every collection that returns a
# co-located heap element, not just arrays.
#
# Mechanism (witnessed under `--trace=guardfree`, bisected to the call-index):
#   free site: `DecrefValueRegion of <value> (runtime region N) @ <the (m :k)>`,
#   the consumer then reads the freed value.
# An immutable struct stores its values as a RegionSlice in the struct's OWN
# region pages (docs/impl/region-model.md § "RegionSlice contents share their object's
# region"), so a co-located heap value has no region of its own — its lifetime
# IS the struct's region. The struct is a let-bound value released value-based at
# its last use (the `(m :k)` expression); that frees the whole struct region,
# value included, while the surrounding native still borrows the value.
#
# WHICH collection-call-index forms are affected (bisected): array `(arr i)` and
# struct `(m :k)` return co-located heap elements and over-release. String
# `(str i)` returns a fresh single-char string (safe); set `(s v)` returns a
# boolean and bytes `(b i)` an int (immediates, no region — safe). So this file
# and region-array-element-uaf.lisp together cover the heap-returning cases.
#
# A UAF, NOT a leak — the witness is a CRASH (regionstore double-free without
# guardfree; SIGSEGV with). RED now on BOTH tiers (interpreter-level: the bug is
# in `call_collection`, shared by --jit=off and the JIT). GREEN once the
# call-index path retains the returned value's region like `get` does.

# ── subjects ──────────────────────────────────────────────────────
# `v` is a heap string the struct holds co-located. `length` borrows the looked-up
# value and returns an immediate, so the only thing that can be freed is the
# co-located value's (== the struct's) region.

# (a) THE trigger: look up a struct key by CALLING the struct, hand the
# co-located value to a borrowing native. The struct's region is released
# value-based at `(m :k)`, freeing the value under length's borrow.
#
# Loop sizing: hundreds of iterations is far past the adaptive-JIT threshold
# (10 calls) while keeping the whole file inside the guardfree mapping budget
# (vm.max_map_count): the oracle leaks one PROT_NONE mapping per FREED region
# page, so a reclaiming stdlib-heavy loop consumes mappings per iteration and
# an oversized count aborts on mmap exhaustion, not a UAF.

(defn via_call (v)
  (let [m (struct :k v)]
    (length (m :k))))

# control: the SAME shape but via `get` — `get` does the pass-through retain, so
# the value survives. Correct NOW; this is the bisection that names the
# call-index path as the culprit (not struct value access in general).
(defn via_get (v)
  (let [m (struct :k v)]
    (length (get m :k))))

# ── witness ───────────────────────────────────────────────────────
# DIRECT calls; a fresh heap value each iteration. The call-index over-releases
# and the next read faults.

(var i 0)
(var last-call 0)
(while (%lt i 500)
  (assign last-call (via_call (concat "a" "b")))
  (assign i (%add i 1)))

(var k 0)
(var last-get 0)
(while (%lt k 500)
  (assign last-get (via_get (concat "a" "b")))
  (assign k (%add k 1)))

# Control: the `get` path retains correctly, so this is correct NOW.
(assert (= last-get 2)
        "control: struct get-index value mis-read (harness broken)")

# Witness: a call-indexed struct value must survive its consuming native.
(assert (= last-call 2) "(m :k) call-index value over-released")

(println "region-struct-call-index-uaf: ok")
