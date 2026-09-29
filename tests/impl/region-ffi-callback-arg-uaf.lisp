(elle/epoch 13)
# audited: 2026-09-29
# An FFI callback that receives several heap arguments gives each its own region, so one release frees no sibling.
# docs/impl/region/rules.md
#
# The libffi trampoline (`trampoline_callback`, src/ffi/callback.rs) converts each
# C argument with `read_value_from_buffer` (src/ffi/from_c.rs). Scalars and
# pointers come back as immediates, with no region. A `:struct` or array argument
# is a fresh heap value (`Value::array`), and a u8 array a fresh `Value::bytes`.
# Each converted heap argument gets a per-execution region of its own, as
# `env_value_region` gives an env value (Rule 6: no commingling). The callback runs
# with move semantics (`build_callback_env` own_params=false), so the callee
# releases each owned parameter by value at its last use.
#
# The counter-factual allocates every argument into one shared region. The first
# parameter's `DecrefValueRegion` then frees the region the second still lives in,
# and the second's release frees it again. Under `--trace=guardfree`:
#   free site: `DecrefValueRegion of array (runtime region N) via direct`,
#   then the sibling arg's release re-frees N (regionstore phantom/double-free).
# Without guardfree it aborts on the regionstore double-free assert. Both tiers
# share the trampoline, so both fail the same way. A single heap argument is safe
# either way, which the controls below show.
#
# The callback bodies use `length`, not `get`, to consume each argument: `length`
# forces the owned-param release and is leak-clean. `(get a 0)` also consumes the
# argument but brings a separate leak (a `get` pass-through result flowing into a
# returned combining expression leaks the container regions, with no FFI
# involved), which would muddy this witness with region growth.

# ── subjects ──────────────────────────────────────────────────────
# `length` on the converted aggregate returns its element count (an immediate),
# forcing the owned-param release; a correct run is verifiable and the over-release
# crashes before the assert is reached.

# (a) CONTROL: a single struct arg — one heap value, its own region, released
# once. Safe either way; it isolates the >1-heap-arg commingle (not struct args
# in general) as the hazard.
(def sig-1struct (ffi/signature :int @[(ffi/struct @[:i32 :i32])]))
(def cb-1struct (ffi/callback sig-1struct (fn (a) (length a))))

# (b) CONTROL: a single array arg — same.
(def sig-1arr (ffi/signature :int @[(ffi/array :i32 2)]))
(def cb-1arr (ffi/callback sig-1arr (fn (a) (length a))))

# (c) WITNESS: TWO struct args. A shared region would take both releases.
(def sig-2struct
  (ffi/signature :int @[(ffi/struct @[:i32 :i32]) (ffi/struct @[:i32 :i32])]))
(def cb-2struct (ffi/callback sig-2struct (fn (a b) (+ (length a) (length b)))))

# (d) WITNESS: TWO array args — same defect via the array conversion path.
(def sig-2arr (ffi/signature :int @[(ffi/array :i32 2) (ffi/array :i32 2)]))
(def cb-2arr (ffi/callback sig-2arr (fn (a b) (+ (length a) (length b)))))

# ── controls: correct (single heap arg, no commingled sibling) ──
(var i 0)
(var c1 0)
(var c2 0)
(while (%lt i 500)
  (assign c1 (ffi/call cb-1struct sig-1struct @[5 0]))
  (assign c2 (ffi/call cb-1arr sig-1arr @[6 0]))
  (assign i (%add i 1)))
(assert (= c1 2) "control: single struct-arg callback mis-read (harness broken)")
(assert (= c2 2) "control: single array-arg callback mis-read (harness broken)")

# ── witnesses: a multi-heap-arg callback must not over-release a sibling ──
# With one shared region the first of these calls double-frees and aborts. With a
# region per argument the results are correct AND the per-arg regions are freed
# (bounded — asserted below).
(def rc-before (arena/region-count))
(var k 0)
(var w1 0)
(var w2 0)
(while (%lt k 3000)
  (assign w1 (ffi/call cb-2struct sig-2struct @[7 0] @[9 0]))
  (assign w2 (ffi/call cb-2arr sig-2arr @[3 0] @[4 0]))
  (assign k (%add k 1)))
(def rc-delta (%sub (arena/region-count) rc-before))

(assert (= w1 4)
        "(struct, struct) callback arg over-released (commingled region)")
(assert (= w2 4) "(array, array) callback arg over-released (commingled region)")

# Each converted heap arg gets its OWN region, freed by the callee — so a long
# run of multi-heap-arg callbacks is bounded, not leaking 2 regions/iteration.
(assert (%lt rc-delta 100)
        (concat "multi-heap-arg callback leaks per-arg regions, delta="
                (number->string rc-delta)))

(ffi/callback-free cb-1struct)
(ffi/callback-free cb-1arr)
(ffi/callback-free cb-2struct)
(ffi/callback-free cb-2arr)

(println "region-ffi-callback-arg-uaf: ok")
