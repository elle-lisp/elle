(elle/epoch 12)
# Oracle: a JIT-COMPILED function's PROLOGUE must mint a fresh per-execution
# region for every env value it builds — capture cells (a mutable-captured param
# or local) and the variadic rest cons-list — exactly as the interpreter's
# `populate_env` does via `env_value_region` / `args_to_list` (src/vm/env.rs).
# docs/impl/region/model.md "RegionSlice contents share their object's region",
# Rule 6 (no commingling), Rule 8 (no leaks).
#
# THE BUG: the JIT body's allocations are each straddled
# by `elle_jit_push_alloc_region(slot)` (a per-execution region), but the
# COMPILED PROLOGUE was not. `src/jit/compiler.rs translate_function` emitted the
# capture-param wraps + the variadic rest cons-loop, and `init_locally_defined_vars`
# (translate.rs) the captured-local cells, via `elle_jit_make_capture` /
# `elle_jit_pair` — which allocate into whatever AMBIENT TLS region the caller
# left set. On a JIT->JIT call the callee inherits the caller's region, so each
# env value commingled with it (Rule 6) and its value-based `DecrefCellRegion` /
# `DecrefValueRegion` decreffed the caller's region — a Rule-8 leak and a latent
# use-after-free. The fix routes the prologue through `elle_jit_make_capture_owned`
# / `elle_jit_collect_rest_list`, which mint a fresh region per value.
#
# WHY THE PRECISE COUNTERFACTUAL IS A RUST UNIT TEST, not this file:
#   - The capture-cell prologue paths are UNREACHABLE from Elle today: a function
#     that captures a param/local necessarily contains a `MakeClosure`, which the
#     JIT rejects (src/jit/compiler.rs ~121), so those prologues only ever run in
#     the interpreter. The fix is latent-correct; the deterministic witness is
#     `src/jit/data.rs` `prologue_capture_cell_gets_its_own_region_not_ambient`.
#   - The variadic rest-list path IS JIT-compiled, but the rest list is freed at
#     its enclosing activation's end on BOTH tiers identically, so no Elle-level
#     count delta isolates the born-in-the-wrong-region defect; the unit test
#     `prologue_rest_list_conses_get_own_regions_not_ambient` pins it directly.
#
# This file is therefore the REACHABLE-path CORRECTNESS regression guard: it
# drives variadic functions JIT->JIT (a compiled driver calling them in a hot
# loop) and asserts their results are correct under JIT — including the empty
# rest-list boundary and the fixed-params+rest split, the exact shapes the
# prologue's `collect_rest_list` rebuilds. A prologue that mis-built the rest
# list (wrong region, dropped/duplicated cons, off-by-one at `non_rest_params`)
# would corrupt these results.
#
# NOTE: this file does NOT assert region/object bounds. The list-rest value
# outlives its last use until its enclosing activation ends on BOTH tiers
# identically (the top-level rest-list-lifetime over-keep of the
# `region-env-leak` family), so a bounds assertion here would be RED on the
# interpreter too and would not isolate the prologue-region defect. Region
# correctness is pinned by the Rust unit tests cited above.

# ── variadic subjects (JIT-compilable: no MakeClosure) ────────────────
(defn vsum (& xs)
  (var s 0)
  (each x xs
    (assign s (+ s x)))
  s)

# fixed params + rest: exercises the prologue's fixed-arg loads AND the rest
# cons-list together (the `non_rest_params` boundary in collect_rest_list).
(defn vtail (a b & xs)
  (%add (%add a b) (length xs)))

# ── driver: when hot, calls the variadics JIT->JIT so their prologue rest
# lists are built under the driver's ambient region (the hazard condition). ──
(defn driver (k)
  (var s 0)
  (var j 0)
  (while (%lt j k)
    (assign s (+ s (vsum 1 2 3 4 5)))
    (assign s (+ s (vtail 10 20 1 2 3)))
    (assign j (%add j 1)))
  s)

# Warmup: drive `driver` (and the variadics it calls) hot so adaptive JIT
# compiles them and later calls go JIT->JIT.
(defn warm (n)
  (var w 0)
  (while (%lt w n)
    (driver 5)
    (assign w (%add w 1))))
(warm 6000)

# ── (a) correctness under JIT ─────────────────────────────────────────
# vsum(1..5)=15, vtail(10,20,1,2,3)=10+20+3=33; one driver iter adds 48.
(assert (= (driver 1) 48) "JIT variadic prologue produced a wrong result")
(assert (= (vsum 1 2 3 4 5) 15) "JIT vsum wrong")
(assert (= (vtail 10 20 1 2 3) 33) "JIT vtail wrong")
(assert (= (vsum) 0) "JIT vsum of no args must be 0 (empty rest list)")
(assert (= (vtail 4 5) 9) "JIT vtail with empty rest list wrong")

# ── (b) sustained correctness under a hot JIT->JIT driver ─────────────
# Run the driver hard so every call is JIT-compiled and going JIT->JIT; the
# accumulated sum must stay exactly right (48 per inner iteration). A prologue
# rest-list corruption surfaces as a wrong total or a crash here.
(assert (= (driver 1000) 48000)
        "JIT variadic prologue corrupted results under a hot JIT->JIT driver")

(println "region-jit-env: ok")
