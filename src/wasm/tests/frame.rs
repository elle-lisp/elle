// audited: 2026-09-06
// docs/impl/wasm.md
//! What a running closure's env and local slots must survive.
//!
//! A closure's env and its local slots are linear memory the emitter addresses
//! by offset, and a value in either can be overwritten by something the
//! emission did elsewhere — a wide call's args region, or a region release's
//! nil-stamp. Both shapes below returned a wrong answer rather than failing.

use super::*;

#[test]
fn wasm_full_wide_call_from_closure_preserves_env() {
    // A call with more args than the fixed args-region window — `(ENV_STACK_BASE
    // - ARGS_BASE) / 16` = 240 slots — made from INSIDE a closure must not
    // clobber that closure's env, which the env-stack allocator lays out at
    // `env_stack_base`. A 250-key struct literal desugars to a 500-arg call to
    // the `struct` primitive; emitted from the body of `f`, its args region
    // `[ARGS_BASE, ARGS_BASE + 500*16)` overruns a fixed 4096-byte env base and
    // corrupts f's param `x` and the freshly-bound `big`. The env stack must
    // begin above the module's widest call. `call-u16.lisp` is the top-level
    // face (no live env below the args, so only the `nargs<=256` guard tripped);
    // this is the in-closure face the fixed window silently corrupted.
    let pairs: String = (0..250)
        .map(|i| format!(":k{i} {i}"))
        .collect::<Vec<_>>()
        .join(" ");
    let src = format!("(defn f [x] (def big {{{pairs}}}) (+ x big:k249))\n(f 1000)");
    assert_eq!(
        eval_with_stdlib(&src),
        "1249",
        "a 500-arg struct call from inside `f` must not clobber f's env — the \
         env stack must begin above the module's widest args region"
    );
}

#[test]
fn wasm_full_reassigned_loop_counter_survives_inner_decref() {
    // A `@`-mutable counter reassigned inside a NESTED loop must not be clobbered
    // by a region decref's nil-stamp. `ii`'s stack slot is its own for its whole
    // scope (`allocate_slot` never reuses a slot), but the region analysis keeps a
    // spurious assign-value region for the immediate-valued `(assign ii (%add ii
    // 1))` and places its `decref_point` inside the inner loop. The lowerer's
    // value-route release would `LoadLocal ii; DecrefValueRegion; StoreLocal ii
    // nil` — nil-stamping the live counter before its own increment reads it, so
    // the emitter's inline `BinOp Add` reads `Nil` as 0 and the loop never
    // terminates. `emit_decrefs_for` refuses the value-route + nil-stamp for a
    // reassigned-local binding's slot (`reassigned_local_slots`), so the counter
    // survives. 2 outer × 3 inner × `(get s 0)`=10 = 60. The full corpus face is
    // `tests/elle/region-capture-cell-loop-uaf.lisp` under `--wasm=full`.
    let src = "\
(defn nested []
  (def @oi 0)
  (def @acc 0)
  (while (%lt oi 2)
    (def @s @[10 20 30])
    (def @ii 0)
    (while (%lt ii 3)
      (let [cl (fn [] (get s 0))]
        (assign acc (+ acc (cl))))
      (assign ii (%add ii 1)))
    (assign oi (%add oi 1)))
  acc)
(nested)";
    assert_eq!(
        eval_with_stdlib(src),
        "60",
        "a reassigned mutable loop counter must not be nil-stamped by an in-loop \
         region decref that names its slot — the loop must terminate"
    );
}
