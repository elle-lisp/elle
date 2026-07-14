(elle/epoch 12)
# Counterfactual: the >64-local capture-cell leak (verona Stage 3 / leak (B)).
#
# `capture_locals_mask` is a `u64`, so it can only name the first 64 locally
# defined slots. The VM env builder (`populate_env`, src/vm/env.rs) and the JIT
# prologue (src/jit/translate.rs) therefore CONSERVATIVELY allocate a
# `CaptureCell` for EVERY local at index >= 64 — even ones no nested closure
# captures — because the mask cannot tell them apart. Those cells have no
# release path (the per-call env-cell class), so a function with N>64 locals
# leaks exactly (N - 64) regions PER CALL.
#
# This is the dominant driver of the stdlib higher-order-function leaks: stdlib
# `map`/`merge`/`group-by` compile to 91/135/91 locals (their sibling-fn calls
# are opaque closure calls, not intrinsics, so they don't collapse the way the
# same source does in user code), leaking 27/71/27 regions per call. `zip`
# (201/call) inherits it by calling `map` ~7 times internally.
#
# The fix widens `capture_locals_mask` to name every local precisely, so an
# UNcaptured local at any index gets a bare-NIL env slot (no cell, no leak),
# while a genuinely captured local at index >= 64 is still celled correctly.
#
# RED before the fix (leaks (locals - 64) regions/call); bounded after.
# docs/impl/region/rules.md Rule 8 ("Nothing leaks but true process-lifetime roots").

# ── a function the compiler gives > 64 local slots ────────────────
# 80 sequential `let*` bindings — each lifts a local; none is captured by a
# nested closure, so NONE genuinely needs a cell. With the u64 mask the env
# builder still cells slots 64..80 (16 dead cells) every call.
(defn many-locals [x]
  (let* [v00 (+ x 1)
         v01 (+ v00 1)
         v02 (+ v01 1)
         v03 (+ v02 1)
         v04 (+ v03 1)
         v05 (+ v04 1)
         v06 (+ v05 1)
         v07 (+ v06 1)
         v08 (+ v07 1)
         v09 (+ v08 1)
         v10 (+ v09 1)
         v11 (+ v10 1)
         v12 (+ v11 1)
         v13 (+ v12 1)
         v14 (+ v13 1)
         v15 (+ v14 1)
         v16 (+ v15 1)
         v17 (+ v16 1)
         v18 (+ v17 1)
         v19 (+ v18 1)
         v20 (+ v19 1)
         v21 (+ v20 1)
         v22 (+ v21 1)
         v23 (+ v22 1)
         v24 (+ v23 1)
         v25 (+ v24 1)
         v26 (+ v25 1)
         v27 (+ v26 1)
         v28 (+ v27 1)
         v29 (+ v28 1)
         v30 (+ v29 1)
         v31 (+ v30 1)
         v32 (+ v31 1)
         v33 (+ v32 1)
         v34 (+ v33 1)
         v35 (+ v34 1)
         v36 (+ v35 1)
         v37 (+ v36 1)
         v38 (+ v37 1)
         v39 (+ v38 1)
         v40 (+ v39 1)
         v41 (+ v40 1)
         v42 (+ v41 1)
         v43 (+ v42 1)
         v44 (+ v43 1)
         v45 (+ v44 1)
         v46 (+ v45 1)
         v47 (+ v46 1)
         v48 (+ v47 1)
         v49 (+ v48 1)
         v50 (+ v49 1)
         v51 (+ v50 1)
         v52 (+ v51 1)
         v53 (+ v52 1)
         v54 (+ v53 1)
         v55 (+ v54 1)
         v56 (+ v55 1)
         v57 (+ v56 1)
         v58 (+ v57 1)
         v59 (+ v58 1)
         v60 (+ v59 1)
         v61 (+ v60 1)
         v62 (+ v61 1)
         v63 (+ v62 1)
         v64 (+ v63 1)
         v65 (+ v64 1)
         v66 (+ v65 1)
         v67 (+ v66 1)
         v68 (+ v67 1)
         v69 (+ v68 1)
         v70 (+ v69 1)
         v71 (+ v70 1)
         v72 (+ v71 1)
         v73 (+ v72 1)
         v74 (+ v73 1)
         v75 (+ v74 1)
         v76 (+ v75 1)
         v77 (+ v76 1)
         v78 (+ v77 1)
         v79 (+ v78 1)]
    v79))

# Sanity: this function really exceeds 64 locals (otherwise the test proves
# nothing — the leak only manifests above the u64 mask boundary).
(assert (< 64 (get (fn/flow many-locals) :locals))
        (string "many-locals must have > 64 locals to exercise the >=64 path; got "
                (get (fn/flow many-locals) :locals)))

(defn highlocal-region-leak [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (many-locals 0)
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

# The leak is whole REGIONS: every dead cell is minted in its own per-execution
# region (env_value_region) that never reaches RC 0. A 2000-call loop over a
# function with no genuinely-captured locals must leak ~0 regions — every local
# uses a stack slot, none needs a cell.
(def d2000 (highlocal-region-leak 2000))
(println "region-highlocal-leak: 2000 calls leaked " d2000 " regions")
(assert (%lt d2000 50)
        (string "a function with > 64 UNcaptured locals must not leak a cell "
                "per high local: 2000 calls leaked " d2000 " regions "
                "(the u64 capture_locals_mask >=64 fallback over-cells)"))

# ── correctness guard: a CAPTURED local at index >= 64 still works ─
# A nested closure captures `acc`, which is bound after > 64 throwaway locals so
# it lands at a slot >= 64. The closure must read and mutate it correctly across
# calls — guards the fix against UNDER-celling a genuine high-index capture.
(defn make-counter-after-many [x]
  (let* [w00 (+ x 1)
         w01 (%add w00 1)
         w02 (%add w01 1)
         w03 (%add w02 1)
         w04 (%add w03 1)
         w05 (%add w04 1)
         w06 (%add w05 1)
         w07 (%add w06 1)
         w08 (%add w07 1)
         w09 (%add w08 1)
         w10 (%add w09 1)
         w11 (%add w10 1)
         w12 (%add w11 1)
         w13 (%add w12 1)
         w14 (%add w13 1)
         w15 (%add w14 1)
         w16 (%add w15 1)
         w17 (%add w16 1)
         w18 (%add w17 1)
         w19 (%add w18 1)
         w20 (%add w19 1)
         w21 (%add w20 1)
         w22 (%add w21 1)
         w23 (%add w22 1)
         w24 (%add w23 1)
         w25 (%add w24 1)
         w26 (%add w25 1)
         w27 (%add w26 1)
         w28 (%add w27 1)
         w29 (%add w28 1)
         w30 (%add w29 1)
         w31 (%add w30 1)
         w32 (%add w31 1)
         w33 (%add w32 1)
         w34 (%add w33 1)
         w35 (%add w34 1)
         w36 (%add w35 1)
         w37 (%add w36 1)
         w38 (%add w37 1)
         w39 (%add w38 1)
         w40 (%add w39 1)
         w41 (%add w40 1)
         w42 (%add w41 1)
         w43 (%add w42 1)
         w44 (%add w43 1)
         w45 (%add w44 1)
         w46 (%add w45 1)
         w47 (%add w46 1)
         w48 (%add w47 1)
         w49 (%add w48 1)
         w50 (%add w49 1)
         w51 (%add w50 1)
         w52 (%add w51 1)
         w53 (%add w52 1)
         w54 (%add w53 1)
         w55 (%add w54 1)
         w56 (%add w55 1)
         w57 (%add w56 1)
         w58 (%add w57 1)
         w59 (%add w58 1)
         w60 (%add w59 1)
         w61 (%add w60 1)
         w62 (%add w61 1)
         w63 (%add w62 1)
         w64 (%add w63 1)
         w65 (%add w64 1)
         w66 (%add w65 1)
         w67 (%add w66 1)
         @acc w67]
    (fn ()
      (assign acc (%add acc 1))
      acc)))

(def c (make-counter-after-many 0))
(assert (= (c) 69) "captured high-index local: first call increments to 69")
(assert (= (c) 70) "captured high-index local: state persists across calls")
(assert (= (c) 71) "captured high-index local: third call")

(println "region-highlocal-leak: ok")
