(elle/epoch 12)
# Counterfactual: a C→Elle FFI callback that receives TWO OR MORE heap-typed
# arguments (`:struct`/array/bytes) over-releases — the converted args were
# commingled into ONE region, so each owned-param value-based release frees that
# shared region while a sibling arg is still live: double-free / use-after-free.
#
# ROOT CAUSE: the libffi trampoline (`trampoline_callback`, src/ffi/callback.rs)
# converts each C argument with `read_value_from_buffer` (src/ffi/from_c.rs).
# Scalars/pointers come back as immediates (no region), but a `:struct`/array arg
# is a FRESH heap value (`Value::array`) and a u8 array a fresh `Value::bytes`.
# The fix mints a fresh per-execution region per converted HEAP arg (mirroring
# `env_value_region`); the BUG was allocating every arg into the AMBIENT TLS
# region, so with >1 heap arg they shared ONE region (docs/impl/region/rules.md Rule 6
# violation: no commingling). The callback is invoked with move/owned-param
# semantics (`build_callback_env` own_params=false: the trampoline's fresh mints
# transfer to the callee, which releases each owned param value-based at its last
# use). So the FIRST param's `DecrefValueRegion` freed the shared region and the
# SECOND param's `DecrefValueRegion` hit an already-freed region — a double-free;
# and any read of the second arg between the two releases is a use-after-free.
#
# Mechanism (witnessed under `--trace=guardfree`):
#   free site: `DecrefValueRegion of array (runtime region N) via direct`,
#   then the sibling arg's release re-frees N (regionstore phantom/double-free).
#
# WHICH callback-arg forms are affected (bisected): `:struct` and array args
# (`read_value_from_buffer` → `Value::array`), and u8 arrays (→ `Value::bytes`)
# — every heap-returning conversion. Scalar/pointer args are immediates (no
# region) and are SAFE. A SINGLE heap arg is also safe: one value alone in its
# region, released exactly once (the controls below prove this).
#
# A UAF/double-free, NOT a leak — the witness was a CRASH (regionstore
# double-free abort without guardfree; SIGSEGV with). RED on BOTH tiers before
# the fix (the bug is in the interpreter-side trampoline, shared by --jit=off and
# the JIT). GREEN once each converted heap arg gets its OWN per-execution region,
# so a per-arg owned release frees only that arg.
#
# NOTE the callback bodies use `length` (not `get`) to consume each arg: `length`
# forces the owned-param release (so the commingle double-free is exercised) and
# is leak-clean. `(get a 0)` would ALSO consume the arg but trips a SEPARATE,
# pre-existing leak (a `get` pass-through result flowing into a returned combining
# expression leaks the container regions — reproducible in plain Elle with no FFI),
# which would muddy this UAF witness with region growth.

# ── subjects ──────────────────────────────────────────────────────
# `length` on the converted aggregate returns its element count (an immediate),
# forcing the owned-param release; a correct run is verifiable and the over-release
# crashes before the assert is reached.

# (a) CONTROL: a single struct arg — one heap value, its own region, released
# once. Safe both before and after; bisects the >1-heap-arg commingle (not struct
# args in general) as the culprit.
(def sig-1struct (ffi/signature :int @[(ffi/struct @[:i32 :i32])]))
(def cb-1struct (ffi/callback sig-1struct (fn (a) (length a))))

# (b) CONTROL: a single array arg — same.
(def sig-1arr (ffi/signature :int @[(ffi/array :i32 2)]))
(def cb-1arr (ffi/callback sig-1arr (fn (a) (length a))))

# (c) WITNESS: TWO struct args. Both releases hit the (formerly) shared region.
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
# Before the fix the first of these ffi/calls double-freed the shared arg region
# and aborted. After: each arg has its own region, so results are correct AND the
# per-arg regions are freed (bounded — asserted below).
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
