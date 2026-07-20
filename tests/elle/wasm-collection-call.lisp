(elle/epoch 12)
# Counterfactual: applying a callable COLLECTION as a function from
# full-module WASM (`--wasm=full`).
#
# THE GAP: the full-module `rt_call` / `rt_prepare_tail_call` host functions
# (src/wasm/linker/create/{call,tailcall}.rs) dispatched only compiled
# closures, bytecode closures, NativeFns, and parameters. A callable
# COLLECTION — a struct/array/set/string/bytes applied as a function, e.g.
# `(struct :key)`, `(arr i)`, `(set x)` — fell through to a `cannot call …`
# type error. The interpreter's `call_inner` routes these through
# `call_collection` (src/vm/call/collection.rs); the WASM linker must too
# (`run_collection_call`, src/wasm/linker.rs), or the two tiers diverge.
#
# WHY IT MATTERS: the async scheduler's request dispatch is built on struct
# application. `make-async-scheduler`'s `handle-wait` reads `(request :op)`
# and `handle-join` reads `(request :fiber)` off the struct a fiber emits via
# `(emit :wait request)`. Only stdlib is compiled into the full module, so
# `handle-wait` runs as compiled WASM and its `(request :op)` reaches
# `rt_call`. Without the collection fallback that call errors, the fiber
# handling silently aborts, and every `ev/join` / `ev/scope` / futex wait
# under `--wasm=full` never resumes — the whole-file WASM pass wraps user
# code in `ev/run`, so this is the scheduler for all concurrency.
#
# RED before the fix under `--wasm=full`: the first `(struct :k)` errors, and
# `(ev/join …)` hangs/aborts, so the marker is never reached and WASM output
# diverges from the VM/JIT tiers. GREEN on every tier once the fallback lands.

# ── struct in call position (rt_call) ──────────────────────────────────
(def s {:a 1 :b 2 :op :join})
(assert (= (s :b) 2) "struct call position")
(assert (= (s :op) :join) "struct call keyword value")
(assert (= (s :missing) nil) "struct call missing key → nil")
(assert (= (s :missing 99) 99) "struct call missing key → default")

# ── struct in TAIL position (rt_prepare_tail_call) ─────────────────────
(defn lookup [m k]
  (m k))
(assert (= (lookup s :a) 1) "struct call in tail position")

# ── array / set / string as functions ─────────────────────────────────
(assert (= ([10 20 30] 1) 20) "array index call")
(assert (= ([10 20 30] -1) 30) "array negative index call")
(def evens (set 2 4 6))
(assert (= (evens 4) true) "set membership call (present)")
(assert (= (evens 5) false) "set membership call (absent)")
(assert (= ("hello" 1) "e") "string grapheme call")

# ── the motivating case: the async scheduler resumes a joined fiber ─────
# `ev/join` emits a `:wait` struct the compiled scheduler dispatches with
# struct application; the child's value must flow back to the joiner.
(assert (= (ev/join (ev/spawn (fn [] 42))) 42) "ev/join returns child value")
(assert (= (ev/join (ev/spawn (fn [] (+ 1 2 3)))) 6) "ev/join computed value")

(println "wasm-collection-call: ok")
