// audited: 2026-10-06
//! What a running closure's env and local slots must survive.
//!
//! docs/impl/wasm.md
//!
//! A closure's env and its local slots are linear memory the emitter addresses
//! by offset, and a value in either can be overwritten by something the
//! emission did elsewhere — a wide call's args region, a park's spill, or a
//! region release's nil-stamp. When its invariant breaks, each shape below
//! returns a wrong answer or fails later on the overwritten value, never at the
//! write.

use super::*;

#[test]
fn wasm_full_wide_call_from_closure_preserves_env() {
    // A call with more args than the fixed args-region window — `(ENV_STACK_BASE
    // - ARGS_BASE) / 16` = 240 slots — made from INSIDE a closure must not
    // clobber that closure's env, which the env-stack allocator lays out at
    // `env_stack_base`. A 250-key struct literal desugars to a 500-arg call to
    // the `struct` primitive; emitted from the body of `f`, its args region
    // `[ARGS_BASE, ARGS_BASE + 500*16)` would overrun a fixed 4096-byte env base and
    // corrupt f's param `x` and the freshly-bound `big`, so the env stack begins
    // above the module's widest call. tests/lang/call-u16.lisp is the top-level
    // face, with no live env below the args; this is the in-closure face.
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
fn wasm_full_wide_park_preserves_env() {
    // A frame that parks spills its saved registers and local slots to
    // `ARGS_BASE`, one 16-byte slot per saved slot, for `rt_yield` to copy out.
    // `wide` holds 1000 values live across its yield, so its spill runs 16 KB
    // past a fixed 4096-byte env base and over the envs of the closures still
    // running below it: `outer`'s param `k` and the scheduler's own captures. The env
    // stack begins above the widest spill as it does above the widest call.
    // The values are body definitions, and the sum is a balanced tree of
    // two-operand adds, so no wide call raises the base. Every add is `%add`
    // over integer literals, which compiles inline: a call would be a park
    // site, and a function of 1000 sites with 1000 live values is past what
    // Wasmtime compiles in a test's time. A debug build's compiler still
    // recurses past a test thread's stack on 1000 definitions, so the program
    // runs on a thread of its own.
    fn sum(lo: usize, hi: usize) -> String {
        if hi - lo == 1 {
            return format!("a{lo}");
        }
        let mid = (lo + hi) / 2;
        format!("(%add {} {})", sum(lo, mid), sum(mid, hi))
    }
    let n = 1000;
    let defs: String = (0..n)
        .map(|i| format!("(def a{i} (%add 1 {i})) "))
        .collect();
    let sum = sum(0, n);
    let src = format!(
        "(defn wide [] {defs}(yield :parked) {sum})\n\
         (defn outer [k] \
           (let [f (fiber/new (fn [] (wide)) |:yield|)] \
             (fiber/resume f) \
             (+ k (fiber/resume f))))\n\
         (outer 7)"
    );
    let shown = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || eval_with_stdlib(&src))
        .expect("spawn")
        .join()
        .expect("the program runs");
    assert_eq!(
        shown, "500507",
        "a park that spills 1000 live slots must not clobber a live env — the env \
         stack must begin above the module's widest spill"
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
    // `tests/impl/region-capture-cell-loop-uaf.lisp` under `--wasm=full`.
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
