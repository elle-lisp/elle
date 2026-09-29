(elle/epoch 12)
# Counterfactual: calling a BYTECODE closure from full-module WASM
# (`--wasm=full`).
#
# THE GAP: `eval_wasm_with_stdlib` (src/wasm/mod.rs) compiles only `stdlib.lisp`
# together with the user source into the wasm module. `core.lisp` and the
# prelude — where `reverse`, `last`, `fold`, `concat`, `append`, and the
# list-processing HOF arms live — are loaded as interpreter bytecode, so their
# closures carry no `wasm_func_idx`. When compiled wasm code calls one (a
# user/stdlib HOF invoking `reverse`; `map`'s list arm, which tail-calls
# `reverse`; a callback bound at runtime), the call reaches the full-module
# `rt_call` / `rt_prepare_tail_call` host functions. These must run the closure
# via the host VM's interpreter (`run_bytecode_closure`, src/wasm/linker.rs);
# without that fallback they raise `{:error :internal-error :message "…bytecode
# closure in WASM backend"}`, which terminates the compiled entry early —
# silently (exit 0) in a small program, or as a hard abort when the error escapes
# through a large top-level thunk's register spill.
#
# This exercises both host paths:
#   - `reverse` / `fold` in ordinary call position    → rt_call
#   - `reverse` in tail position of a user function    → rt_prepare_tail_call
#   - `map` over a LIST (its recursive arm tail-calls `reverse`)
#
# The array arm of `map` compiles entirely (a `while` loop, no bytecode-closure
# call), so it is the control that always worked — the list arm is the one this
# pins.
#
# RED before the fix under `--wasm=full`: the first bytecode-closure call errors
# and the program never reaches the final marker, so wasm output diverges from
# the VM/JIT tiers (and the asserts, once reached post-fix, guard the values).
# GREEN on every tier once `run_bytecode_closure` is wired in.

# ── core.lisp bytecode closure in call position (rt_call) ──────────────
(assert (= (reverse (list 1 2 3 4)) (list 4 3 2 1)) "reverse a list (rt_call)")
(assert (= (last (list 1 2 3)) 3) "last of a list (rt_call)")
(assert (= (fold (fn [a x] (+ a x)) 0 (list 1 2 3 4 5)) 15) "fold over a list")
(assert (= (concat (list 1 2) (list 3 4)) (list 1 2 3 4)) "concat two lists")

# ── bytecode closure in TAIL position (rt_prepare_tail_call) ───────────
(defn tail-reverse [xs]
  (reverse xs))
(assert (= (tail-reverse (list 5 6 7)) (list 7 6 5)) "reverse in tail position")

# ── map over a list: its recursive arm tail-calls `reverse` ────────────
(assert (= (map (fn [x] (* x x)) (list 1 2 3 4)) (list 1 4 9 16))
        "map a closure over a list")
# array arm is the control (compiles fully, no bytecode-closure call)
(assert (= (map (fn [x] (* x x)) [1 2 3 4]) [1 4 9 16])
        "map a closure over an array")

(println "wasm-bytecode-closure-call: ok")
