(elle/epoch 12)
# Counterfactual: calling an ARRAY as a function to index it — `(arr i)` — returns
# the co-located ELEMENT without a pass-through retain on the array's region, so
# releasing the array frees the element under its consumer's borrow: use-after-free.
#
# This is the ARRAY sibling of region-struct-call-index-uaf.lisp and
# region-mut-collection-call-index-uaf.lisp. Same root defect: the
# collection-as-function call-index path (`call_collection`, src/vm/call.rs) did
# NOT incref the returned element's region the way `get`/`first` DO
# (docs/impl/region-rules.md Rule 5, native-result pass-through). The fix routes every
# collection call-index through `VM::dispatch_collection_call`, which mints the
# per-execution result region and applies the pass-through retain exactly like
# `dispatch_native_call` does for `get`/`first` — interpreter AND JIT, tail AND
# non-tail.
#
# Mechanism (witnessed under `--trace=guardfree`, bisected to the call-index):
#   free site: `DecrefValueRegion of array (runtime region N) @ <the (a i)>`,
#   the consumer then reads the freed element.
# An array's payload is an immutable RegionSlice in the array's OWN region pages
# (docs/impl/region-model.md § "RegionSlice contents share their object's region"), so an
# element has no region of its own — its lifetime IS the array's region. The
# array is a let-bound value released value-based at its last use (the `(a i)`
# expression); without the retain that frees the whole array region, element
# included, while the surrounding native still borrows the element.
#
# CONSUMER CHOICE (matches the struct/mut siblings): `length` borrows the
# looked-up element and returns an IMMEDIATE, so the only region that can be
# freed under the borrow is the element's (== the array's). A consumer that
# itself RETURNS a heap result (e.g. `string/trim`) additionally trips a separate
# native-tail-return-of-heap-value UAF (see region-native-tail-return-uaf.lisp),
# which is NOT the call-index defect — `get`/`first` fault there too. Using
# `length` here isolates exactly the call_collection pass-through retain.
#
# A UAF, NOT a leak — the witness is a CRASH (`regionstore.rs:172` double-free
# without guardfree; SIGSEGV with). RED before the fix; GREEN once the call-index
# path retains the returned element's region like `get` does. Bisected: the `get`
# controls are correct NOW (they perform the pass-through retain).

# ── subjects ──────────────────────────────────────────────────────
# `v` is a heap string the array holds co-located. `length` borrows the looked-up
# element and returns an immediate.

# (a) immutable array LITERAL, indexed by CALLING it.
#
# Loop sizing: hundreds of iterations is far past the adaptive-JIT threshold
# (10 calls) while keeping the whole file inside the guardfree mapping budget
# (vm.max_map_count): the oracle leaks one PROT_NONE mapping per FREED region
# page, so a reclaiming stdlib-heavy loop consumes mappings per iteration and
# an oversized count aborts on mmap exhaustion, not a UAF.

(defn call_arr (v)
  (let [a [v]]
    (length (a 0))))

# (b) array produced by a native (`string/split`), indexed by CALLING it — the
# minimal form of the dns/parse-resolv-conf `(parts 1)` shape, isolated to the
# call-index (the higher-order map/filter/freeze chain trips a separate
# filter-over-array UAF; see region-filter-array-uaf.lisp).
(defn call_split (s)
  (let [parts (string/split s " ")]
    (length (parts 1))))

# controls: the SAME shapes via `get` — `get` does the pass-through retain, so the
# element survives. Correct NOW; the bisection that names the call-index path as
# the culprit (not element access in general).
(defn get_arr (v)
  (let [a [v]]
    (length (get a 0))))

(defn get_split (s)
  (let [parts (string/split s " ")]
    (length (get parts 1))))

# ── witness ───────────────────────────────────────────────────────
# DIRECT calls; a fresh heap value each iteration. The call-index over-releases
# and the next read faults.

(var i 0)
(var la 0)
(var lp 0)
(while (%lt i 500)
  (assign la (call_arr (concat "ab" "c")))
  (assign lp (call_split "aa bbb c"))
  (assign i (%add i 1)))

(var k 0)
(var ga 0)
(var gp 0)
(while (%lt k 500)
  (assign ga (get_arr (concat "ab" "c")))
  (assign gp (get_split "aa bbb c"))
  (assign k (%add k 1)))

# Controls: the `get` paths retain correctly, so these are correct NOW.
(assert (= ga 3) "control: array get-index element mis-read (harness broken)")
(assert (= gp 3) "control: split get-index element mis-read (harness broken)")

# Witnesses: a call-indexed element must survive its consuming native.
(assert (= la 3)
        "(a i) call-index element over-released — array region freed under length's borrow")
(assert (= lp 3)
        "(parts i) call-index element over-released — split-array region freed under the borrow")

(println "region-array-element-uaf: ok")
