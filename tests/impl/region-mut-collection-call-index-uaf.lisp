(elle/epoch 12)
# audited: 2026-09-29
# Calling a mutable collection to index it retains the element it returns, as get does, so no cascade frees it early.
# docs/impl/region/rules.md
#
# The mutable siblings of region-array-element-uaf.lisp (immutable array) and
# region-struct-call-index-uaf.lisp (immutable struct). The collection-as-function
# call-index path (`call_collection`, src/vm/call/collection.rs) increfs the returned
# element's region the way `get`/`first` do (Rule 5, native-result pass-through),
# and every collection that hands back a heap element needs it — the two
# immutable forms AND these two mutable forms.
#
# Mechanism (mutable case): a mutable container stores its values by reference,
# increfing the stored value's region at the store (Rule 5, mutable store). So the
# value has its OWN region, kept alive by the container's stored reference. The
# counter-factual returns that value from `( i)` / `( :k)` with NO incref:
# when the container is released value-based at its last use (the call-index
# expression), the cascade decrefs the value's region to 0 and frees it while
# the surrounding native still borrows it. (In the immutable case the element
# shares the container's own region, but the missing retain is the same.)
#
# The witness is a crash, not a leak: a regionstore double-free without
# guardfree, a SIGSEGV with it. The path is the interpreter's, shared by both
# tiers. The `get` controls take the pass-through retain either way.

# ── subjects ──────────────────────────────────────────────────────
# `v` is a heap string the container stores. `length` borrows the looked-up value
# and returns an immediate, so the only thing that can be freed is the value's
# region.

# (a) @array call-index
#
# Loop sizing: hundreds of iterations is far past the adaptive-JIT threshold
# (10 calls) while keeping the whole file inside the guardfree mapping budget
# (vm.max_map_count): the oracle leaks one PROT_NONE mapping per FREED region
# page, so a reclaiming stdlib-heavy loop consumes mappings per iteration and
# an oversized count aborts on mmap exhaustion, not a UAF.

(defn call_arr (v)
  (let [a @[v]]
    (length (a 0))))

# (b) @struct call-index
(defn call_struct (v)
  (let [m @{:k v}]
    (length (m :k))))

# controls: the SAME shape via `get` — `get` retains, so the value survives.
(defn get_arr (v)
  (let [a @[v]]
    (length (get a 0))))

(defn get_struct (v)
  (let [m @{:k v}]
    (length (get m :k))))

# ── witness ───────────────────────────────────────────────────────
# DIRECT calls; a fresh heap value each iteration. The call-index over-releases
# and the next read faults.

(var i 0)
(var la 0)
(var ls 0)
(while (%lt i 500)
  (assign la (call_arr (concat "a" "b")))
  (assign ls (call_struct (concat "a" "b")))
  (assign i (%add i 1)))

(var k 0)
(var ga 0)
(var gs 0)
(while (%lt k 500)
  (assign ga (get_arr (concat "a" "b")))
  (assign gs (get_struct (concat "a" "b")))
  (assign k (%add k 1)))

# Controls: the `get` paths retain, so these hold whatever the call-index does.
(assert (= ga 2) "control: @array get-index value mis-read (harness broken)")
(assert (= gs 2) "control: @struct get-index value mis-read (harness broken)")

# Witnesses: a call-indexed mutable-collection value must survive its native.
(assert (= la 2) "(@arr i) call-index value over-released")
(assert (= ls 2) "(@m :k) call-index value over-released")

(println "region-mut-collection-call-index-uaf: ok")
