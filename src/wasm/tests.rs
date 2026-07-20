//! Emit-size invariants for the CPS suspend/resume machinery, the standalone
//! emission gate, and the backend-tier region gauge pins.
//!
//! A suspending WASM function spills live state before a yield/suspending-call
//! and restores it on resume. The code for this must scale with the *live* set
//! at each suspend point, not with `suspend_points × total_slots`. When it
//! doesn't, a large suspending stdlib function (e.g. a string builder that
//! happens to be marked `may_suspend`) emits a multi-megabyte function body
//! that exceeds Wasmtime's per-function size limit and the whole module fails
//! to parse. These tests pin the linear-in-slots invariant.

use super::emit::{emit_module, emit_single_closure};
use crate::lir::{
    BasicBlock, Label, LirConst, LirFunction, LirInstr, LirModule, Reg, SpannedInstr,
    SpannedTerminator, Terminator,
};
use crate::signals::{Signal, SIG_YIELD};
use crate::syntax::Span;
use crate::value::Arity;

fn spanned(instr: LirInstr) -> SpannedInstr {
    SpannedInstr::new(instr, Span::synthetic())
}

fn block(label: u32, instrs: Vec<LirInstr>, term: Terminator) -> BasicBlock {
    let mut b = BasicBlock::new(Label(label));
    b.instructions = instrs.into_iter().map(spanned).collect();
    b.terminator = SpannedTerminator::new(term, Span::synthetic());
    b
}

/// A trivial non-suspending entry that just returns nil, so the module has a
/// valid entry function alongside the closure under test.
fn trivial_entry() -> LirFunction {
    let mut f = LirFunction::new(Arity::Exact(0));
    f.num_regs = 1;
    f.blocks = vec![block(
        0,
        vec![LirInstr::Const {
            dst: Reg(0),
            value: LirConst::Nil,
        }],
        Terminator::Return(Reg(0)),
    )];
    f
}

/// A suspending closure with `n_yields` yield points and `n_locals` declared
/// local slots that are never read (dead). Value register `Reg(0)` is defined
/// once and carried across every yield to the final return, so it stays live
/// throughout. Because the locals are dead, a live-aware emitter spills none of
/// them; a slot-count-blind emitter spills and restores all of them at every
/// one of the `n_yields` points.
fn suspending_closure(n_yields: u32, n_locals: u16) -> LirFunction {
    let mut f = LirFunction::new(Arity::Exact(1));
    f.closure_id = Some(crate::lir::ClosureId(0));
    f.num_regs = 1;
    f.num_locals = n_locals;
    f.num_params = 1;
    f.signal = Signal::yields();

    let mut blocks = Vec::new();
    // Block 0 defines the carried value, then yields to block 1.
    blocks.push(block(
        0,
        vec![LirInstr::Const {
            dst: Reg(0),
            value: LirConst::Int(1),
        }],
        Terminator::Emit {
            signal: SIG_YIELD,
            value: Reg(0),
            resume_label: Label(1),
        },
    ));
    // Middle yield blocks, each carrying Reg(0) forward.
    for i in 1..n_yields {
        blocks.push(block(
            i,
            vec![],
            Terminator::Emit {
                signal: SIG_YIELD,
                value: Reg(0),
                resume_label: Label(i + 1),
            },
        ));
    }
    // Final block returns the carried value.
    blocks.push(block(n_yields, vec![], Terminator::Return(Reg(0))));
    f.blocks = blocks;
    f
}

/// Emit a module whose single closure is `func`, returning the module bytes.
fn emit_bytes(func: LirFunction) -> Vec<u8> {
    let vm = crate::vm::VM::new();
    let module = LirModule {
        entry: trivial_entry(),
        closures: vec![func],
    };
    emit_module(
        &module,
        std::collections::HashSet::new(),
        vm.heap_ptr,
        std::ptr::null_mut(),
    )
    .wasm_bytes
}

#[test]
fn resume_and_spill_are_linear_in_slots_not_states_times_slots() {
    // 80 yield points, 150 dead local slots. With per-state dense restore and
    // dense local spill the body is ~O(states × slots) ≈ hundreds of KB; with
    // hoisted restore and live-aware local spill it is ~O(states + slots).
    let bytes = emit_bytes(suspending_closure(80, 150));

    // Generous ceiling: the linear emission is a few KB of dispatch + one
    // restore of the declared slots. The quadratic emission is ~280KB. 40KB
    // sits well above the linear size and well below the quadratic one.
    assert!(
        bytes.len() < 40_000,
        "suspending closure emitted {} bytes — spill/restore is scaling with \
         states × slots instead of live slots (CPS size blowup)",
        bytes.len()
    );
}

#[test]
fn dead_locals_do_not_multiply_by_state_count() {
    // The core invariant: adding local slots that are dead across every suspend
    // point must add O(slots) to the module (they are declared and reloaded once
    // by the hoisted restore), never O(states × slots) (spilled AND restored at
    // every one of the suspend points). Hold the state count fixed and grow the
    // dead-local count by 500.
    let few = emit_bytes(suspending_closure(100, 20)).len();
    let many = emit_bytes(suspending_closure(100, 520)).len();
    // 500 extra dead locals across 100 states. Live-aware, hoisted emission adds
    // a few KB (one restore of the extra slots + their declarations); the
    // states × slots emission would add ~100 × 500 slots of spill + restore,
    // on the order of a megabyte.
    assert!(
        many - few < 60_000,
        "500 extra dead locals grew the module by {} bytes at 100 states — \
         spill/restore is scaling with states × slots, not live slots",
        many - few
    );
}

// ── The standalone emission gate ─────────────────────────────────────
//
// A standalone single-closure module is served by hosts whose suspension and
// tail-call imports are panic stubs (lazy/env.rs) and whose funcref table has
// one entry, so `emit_single_closure` must refuse every shape whose execution
// would reach one of them (src/wasm/AGENTS.md § "Constraints on per-closure
// compilation"). Refusal means `None`: the tiered caller falls back to the
// bytecode VM, the precache caller to full-module dispatch.

fn static_region(id: u32) -> crate::hir::region::StaticRegion {
    crate::hir::region::StaticRegion::new(id).expect("nonzero static slot")
}

/// A closure whose block carries one tail call (callee register is arbitrary —
/// the gate is structural, it never resolves the callee).
fn tail_calling_closure() -> LirFunction {
    let mut f = LirFunction::new(Arity::Exact(1));
    f.closure_id = Some(crate::lir::ClosureId(0));
    f.num_regs = 2;
    f.num_params = 1;
    f.blocks = vec![block(
        0,
        vec![
            LirInstr::Const {
                dst: Reg(0),
                value: LirConst::Int(1),
            },
            LirInstr::TailCall {
                dst: Reg(1),
                func: Reg(0),
                args: vec![],
                arity_checked: false,
                region: static_region(2),
                defer_callee_release: false,
                deferred_release_slot: None,
            },
        ],
        Terminator::Return(Reg(1)),
    )];
    f
}

/// A closure that constructs a nested closure (`MakeClosure`), resolvable only
/// with module context.
fn nested_closure_closure() -> LirFunction {
    let mut f = LirFunction::new(Arity::Exact(1));
    f.closure_id = Some(crate::lir::ClosureId(1));
    f.num_regs = 1;
    f.num_params = 1;
    f.blocks = vec![block(
        0,
        vec![LirInstr::MakeClosure {
            dst: Reg(0),
            closure_id: crate::lir::ClosureId(0),
            captures: vec![],
            region: static_region(2),
        }],
        Terminator::Return(Reg(0)),
    )];
    f
}

/// A plain numeric closure — the positive control proving the gate is not
/// over-broad.
fn plain_closure() -> LirFunction {
    let mut f = LirFunction::new(Arity::Exact(1));
    f.closure_id = Some(crate::lir::ClosureId(0));
    f.num_regs = 1;
    f.num_params = 1;
    f.blocks = vec![block(
        0,
        vec![LirInstr::Const {
            dst: Reg(0),
            value: LirConst::Int(7),
        }],
        Terminator::Return(Reg(0)),
    )];
    f
}

#[test]
fn standalone_emission_admits_plain_closures() {
    let vm = crate::vm::VM::new();
    assert!(
        emit_single_closure(&plain_closure(), None, vm.heap_ptr, std::ptr::null_mut()).is_some(),
        "a numeric closure with no stub-reaching shape must be standalone-emittable"
    );
}

#[test]
fn standalone_emission_refuses_suspending_closures() {
    // An Emit terminator — yield and `(error …)` alike — routes through
    // rt_yield's suspension-frame machinery, a panic stub outside the
    // full-module store.
    let vm = crate::vm::VM::new();
    assert!(
        emit_single_closure(
            &suspending_closure(1, 0),
            None,
            vm.heap_ptr,
            std::ptr::null_mut()
        )
        .is_none(),
        "a closure with an Emit terminator compiled standalone would panic at \
         the tiered host's rt_yield stub"
    );
}

#[test]
fn standalone_emission_refuses_tail_calls() {
    // return_call_indirect needs callee funcref-table indices and
    // rt_prepare_tail_call — a panic stub, and a 1-entry table.
    let vm = crate::vm::VM::new();
    assert!(
        emit_single_closure(
            &tail_calling_closure(),
            None,
            vm.heap_ptr,
            std::ptr::null_mut()
        )
        .is_none(),
        "a closure with a TailCall compiled standalone would panic at the \
         tiered host's rt_prepare_tail_call stub"
    );
}

#[test]
fn standalone_emission_refuses_module_less_make_closure() {
    // ClosureId resolution needs the module's closure list; with it the shape
    // is admitted (the precache path), without it refused (the tiered path).
    let vm = crate::vm::VM::new();
    assert!(
        emit_single_closure(
            &nested_closure_closure(),
            None,
            vm.heap_ptr,
            std::ptr::null_mut()
        )
        .is_none(),
        "MakeClosure without module context has no ClosureId resolution"
    );
    let module = LirModule {
        entry: trivial_entry(),
        closures: vec![plain_closure(), nested_closure_closure()],
    };
    assert!(
        emit_single_closure(
            &nested_closure_closure(),
            Some(&module),
            vm.heap_ptr,
            std::ptr::null_mut()
        )
        .is_some(),
        "MakeClosure with module context resolves through rt_make_closure"
    );
}

// ── Compile-time macro expansion resolves stdlib ─────────────────────
//
// The full-module WASM path (`eval_wasm_with_stdlib`) splices stdlib SOURCE
// into the compiled unit so stdlib functions become WASM at runtime. But macro
// expansion still runs at COMPILE time on the context's macro VM, and a prelude
// macro's transformer body may call a stdlib function: `assert`'s transformer
// (src/prelude.lisp) calls `pair?` (a stdlib `defn`, src/stdlib.lisp) to decide
// whether the asserted form is a comparison. If the compile context's macro VM
// never loaded stdlib, that call resolves to nothing and expansion dies with
// "undefined variable: pair?" — which took down nearly every corpus file under
// `--wasm=full`, since virtually all of them use `assert`. These pin that the
// full-module path loads stdlib into the macro-expansion environment.

/// Run stdlib-backed source through the full-module WASM backend and return the
/// result's display form. `eval_wasm_*` materializes that string while its
/// per-call heap is alive, so a compound result (list, string, mutable) is
/// returned safely — not left dangling past the heap's teardown.
fn eval_with_stdlib(source: &str) -> String {
    match super::eval_wasm_with_stdlib(source, "<macro-stdlib>") {
        Ok(s) => s,
        Err(e) => panic!("eval_wasm_with_stdlib failed: {}", e),
    }
}

#[test]
fn wasm_full_expands_assert_macro() {
    // `(assert true)` is the minimal trigger: `assert`'s transformer calls the
    // stdlib `pair?` at expansion time. Before the macro VM loaded stdlib this
    // failed to compile at all. The asserted form is truthy, so it yields `true`.
    assert_eq!(
        eval_with_stdlib("(assert true)"),
        "true",
        "assert's transformer calls the stdlib `pair?`; the full-module WASM \
         path must load stdlib into the macro-expansion environment"
    );
}

#[test]
fn wasm_full_expands_macro_calling_stdlib_function() {
    // The defect generalizes past `assert`: ANY user macro whose transformer
    // body calls a stdlib function must expand. This one branches on `pair?` of
    // its literal argument — a compile-time stdlib call independent of prelude.
    assert_eq!(
        eval_with_stdlib("(defmacro m [x] (if (pair? x) `1 `2))\n(m (a b))"),
        "1",
        "a user macro calling the stdlib `pair?` at expansion time must resolve \
         it under the full-module WASM path"
    );
}

#[test]
fn wasm_full_bakes_quoted_symbol_literal() {
    // A compound quoted literal with a *symbol* leaf (`(= a 2)` is a list of the
    // symbols `=`, `a` and the int `2`) reaches the emitter as a
    // `MaterializeConst` of a `ConstTemplate::Pair(...Symbol...)`. Baking it into
    // the const pool interns each symbol into the driving instance's table; with
    // no table threaded, `materialize` panicked ("no symbol table for a quoted
    // symbol"). Pins the WasmEmitter::symbols wiring. Reducing to a bool with
    // `=` proves the baked symbol interned to the SAME id the reader gives a
    // fresh `=` — and keeps the returned value immediate (see `eval`'s caveat).
    assert_eq!(
        eval_with_stdlib("(= (first (quote (= a 2))) (quote =))"),
        "true",
        "a quoted compound literal's symbol leaves must bake into the const pool \
         and intern to the reader's ids under the full-module WASM path"
    );
}

#[test]
fn wasm_full_expands_comparison_assert() {
    // The realistic corpus shape: `(assert (= L R))` takes `assert`'s comparison
    // branch, which embeds `(quote (= 1 1))` — a compound SYMBOL literal — into
    // the expansion. It exercises BOTH defects at once: the transformer calls the
    // stdlib `pair?` (macro-expansion resolution) AND the expansion bakes a
    // quoted-symbol literal (const-pool interning). Nearly every corpus file uses
    // this form, so it is what took the `--wasm=full` pass down. Truthy → `true`.
    assert_eq!(
        eval_with_stdlib("(assert (= 1 1) \"one equals one\")"),
        "true",
        "a comparison `assert` must both expand (stdlib `pair?`) and bake its \
         quoted-form literal under the full-module WASM path"
    );
}

// ── Spawning a WASM-built closure to an OS-thread VM worker ───────────
//
// `sys/spawn`/`sys/spawn-vm` deep-copy a closure to a fresh OS-thread bytecode
// VM and run its `template.code()` there (src/primitives/concurrency.rs). Under
// `--wasm=full` the closure is built by `rt_make_closure`, which reconstructs a
// `ClosureTemplate` from the module's dual-compiled bytecode. That bytecode's
// `MakeClosure` instructions index into the template's `child_protos` (the
// nested-lambda blueprints), so the reconstruction MUST carry them: without
// them the worker's first `MakeClosure` indexes an empty list and panics
// (`child_protos[idx]`, src/vm/closure.rs). The corpus spawn/concurrency files
// (concurrency.lisp, send-lir.lisp, region-spawn-*.lisp …) all hit this. These
// return an immediate int (see `eval_with_stdlib`'s caveat); a worker failure
// aborts the join, so the value diverges from the expected int.

#[test]
fn wasm_full_spawn_runs_closure_referencing_children() {
    // `(+ 100 1)` compiles to a body whose dual-compiled bytecode references the
    // template's children; before child_protos were carried, the worker panicked
    // on its first MakeClosure. Joining the worker must yield the sum.
    assert_eq!(
        eval_with_stdlib("(sys/join (sys/spawn-vm (fn () (+ 100 1))))"),
        "101",
        "a WASM-built closure spawned to an OS-thread VM worker must carry its \
         child prototypes so the worker can run it"
    );
}

#[test]
fn wasm_full_spawn_runs_nested_closure() {
    // A spawned closure that itself builds a nested closure (`g`) exercises the
    // MakeClosure → child_protos path directly. Joining must yield g's result.
    assert_eq!(
        eval_with_stdlib("(sys/join (sys/spawn-vm (fn () (let [g (fn [x] (* x x))] (g 6)))))"),
        "36",
        "a spawned WASM-built closure that constructs a nested closure must \
         resolve it through the carried child prototypes"
    );
}

// ── Callable collections dispatch host-side ──────────────────────────
//
// A struct/array/set/string/bytes applied as a function — `(struct :k)`,
// `(arr i)`, `(set x)` — is a callable collection: the interpreter routes it
// through `call_collection` (src/vm/call/collection.rs). Only stdlib compiles
// into the full module, so such a call reaches the `rt_call` /
// `rt_prepare_tail_call` host functions, which must run the same fallback
// (`run_collection_call`) or the value falls through to a `cannot call …`
// error. The async scheduler makes this load-bearing: `make-async-scheduler`'s
// `handle-wait` reads `(request :op)` off the struct a fiber emits with
// `(emit :wait request)`, so without the fallback no `ev/join` under
// `--wasm=full` ever resumes. The corpus twin is
// tests/elle/wasm-collection-call.lisp (VM/JIT divergence + the marker).

#[test]
fn wasm_full_calls_struct_as_function() {
    // `(s :b)` in call position and `(m k)` in the tail position of a helper
    // both reach the host call functions with a struct callee.
    assert_eq!(
        eval_with_stdlib("(let [s {:a 1 :b 2}] (s :b))"),
        "2",
        "a struct applied as a function must index host-side under --wasm=full"
    );
    assert_eq!(
        eval_with_stdlib("(defn lookup [m k] (m k))\n(lookup {:a 1 :b 2} :a)"),
        "1",
        "a struct call in tail position must index via rt_prepare_tail_call"
    );
    assert_eq!(
        eval_with_stdlib("(let [s {:a 1}] (s :missing 99))"),
        "99",
        "a struct call's 2-arg form returns the default for a missing key"
    );
}

#[test]
fn wasm_full_calls_array_set_string_as_function() {
    assert_eq!(
        eval_with_stdlib("([10 20 30] 1)"),
        "20",
        "an array applied as a function must index host-side"
    );
    assert_eq!(
        eval_with_stdlib("(= ((set 2 4 6) 4) true)"),
        "true",
        "a set applied as a function must test membership host-side"
    );
    assert_eq!(
        eval_with_stdlib("(= (\"hello\" 1) \"e\")"),
        "true",
        "a string applied as a function must index a grapheme host-side"
    );
}

#[test]
fn wasm_full_tail_io_in_fiber_delivers_result() {
    // A fiber whose final action is a tail-position io (`ev/sleep`) must have
    // the io submitted by the scheduler and complete, not be re-queued and
    // resumed with a stale nil. `rt_prepare_tail_call` writes the native's
    // SIG_IO to memory; `handle_wasm_result` must OR (not replace) SIG_YIELD so
    // the scheduler still sees SIG_IO on fiber/bits. `ev/sleep` completes nil.
    assert_eq!(
        eval_with_stdlib("(ev/join (ev/spawn (fn [] (ev/sleep 0.001))))"),
        "nil",
        "a fiber ending in a tail-position io must complete under --wasm=full"
    );
}

#[test]
fn wasm_full_wait_via_call_resumes_continuation() {
    // A fiber that suspends on a structured-concurrency wait THROUGH a function
    // call (`ev/join`, whose signal narrows to SIG_WAIT without SIG_YIELD) must
    // resume into the code after the call. The LIR must mark a SIG_WAIT call as
    // suspending (a CPS continuation frame); keyed off SIG_YIELD alone, the
    // continuation `(+ x 2)` was dropped and the fiber returned the wait result.
    assert_eq!(
        eval_with_stdlib(
            "(ev/join (ev/spawn (fn [] (let [x (ev/join (ev/spawn (fn [] 1)))] (+ x 2)))))"
        ),
        "3",
        "a wait-via-call must resume its continuation under --wasm=full"
    );
}

#[test]
fn wasm_full_scheduler_resumes_joined_fiber() {
    // The motivating case: `ev/join` emits a `:wait` struct the compiled
    // scheduler dispatches with struct application; the child's value must flow
    // back through `handle-join` to the joiner. Before the collection fallback,
    // `handle-wait`'s `(request :op)` errored and the join never resumed.
    assert_eq!(
        eval_with_stdlib("(ev/join (ev/spawn (fn () 42)))"),
        "42",
        "the async scheduler must resume a joined fiber under --wasm=full — its \
         request dispatch is built on struct-as-function application"
    );
}

// ── Top-level def semantics match the VM ─────────────────────────────
//
// A file's top level uses sequential shadowing, so redefining a top-level `def`
// is a redefinition (the RHS sees the previous binding), not an error — a
// language feature the corpus relies on (tests/elle/def-shadow.lisp). The naive
// full-module wrap put the whole user body in one `(fn [] …)`, making those defs
// a fn-body letrec* where a duplicate binding is rejected, so def-shadow/numeric/…
// failed to compile under `--wasm=full`.
//
// `build_full_source` now branches on `has_toplevel_redefinition`: a program
// that redefines a top-level name is restructured (definitions at the file top
// level, expression runs under `ev/run`; `build_scheduled_toplevel`) to match
// the VM; every other program keeps the single-thunk wrap, which preserves
// closure execution context for the whole body — needed because a top-level
// def's RHS runs in the ENTRY function, where `eval`'s dynamic compilation traps
// while working in a closure.

#[test]
fn wasm_full_allows_toplevel_def_redefinition() {
    // `(def a 10)` then `(def a (+ a 1))` — the redefinition's RHS reads the
    // previous `a`. Rejected as a duplicate binding when nested in a thunk;
    // allowed at the file top level. This program redefines `a`, so it takes the
    // restructure path. Trailing `a` is the program's value.
    assert_eq!(
        eval_with_stdlib("(def a 10)\n(def a (+ a 1))\na"),
        "11",
        "top-level def redefinition must use sequential shadowing under \
         --wasm=full, as it does on the VM"
    );
}

// The single-wrap-preserves-`eval` behavior (a non-redefining program whose
// top-level def RHS calls `eval`, which traps in the entry function but works in
// a closure) is pinned by tests/elle/region-termination-sweep.lisp and
// tests/elle/region-eval-quoted-data-leak.lisp under `--wasm=full`, not a unit
// test here: `eval`'s wasm compile-context teardown segfaults the in-process
// test harness on drop, though it is clean under the CLI's process exit.

#[test]
fn wasm_full_interleaved_defs_and_expression_runs_emit() {
    // Under the restructure path (this program redefines `s`), interleaved defs
    // and expressions become several `(ev/run (fn [] …))` thunks — each a
    // suspending closure — followed by a short entry. Closures emit before the
    // entry, and a suspending closure leaves resume continuations pointing into
    // ITS blocks; the entry must reset that state or emit_cfg slices the entry's
    // own shorter block at a stale offset and panics (src/wasm/controlflow.rs,
    // was tests/elle/bug-propagate-free-at.lisp under --wasm=full). The trailing
    // (length (pairs …)) is 0 — an immediate, safe to return per `eval`'s caveat.
    assert_eq!(
        eval_with_stdlib(
            "(def s @{:a (or nil {})})\n\
             (assert (= (type-of (get s :a)) :struct) \"a\")\n\
             (def s @{:a (or nil {})})\n\
             (assert (= (type-of (get s :a)) :struct) \"b\")\n\
             (def s @{:a (or nil {})})\n\
             (assert (= (type-of (get s :a)) :struct) \"c\")\n\
             (println (type-of (get s :a)))\n\
             (println (type-of (get s :a)))\n\
             (println (type-of (get s :a)))\n\
             (println (type-of (get s :a)))\n\
             (println (type-of (get s :a)))\n\
             (println (type-of (get s :a)))\n\
             (length (pairs (get s :a)))"
        ),
        "0",
        "interleaved defs and expression runs must emit without carrying stale \
         resume continuations into the entry function"
    );
}

#[test]
fn wasm_full_toplevel_defs_still_run_under_scheduler() {
    // On the restructure path (this program redefines `n`), keeping defs
    // top-level must not lose the scheduler. `sys/join`'s deadline is
    // scheduler-cooperative (chan/select) and fails outside `ev/run` — the exact
    // reason concurrency.lisp needs the wrap — so a passing join proves the
    // expression run executes under the scheduler. The redefined top-level `n` is
    // read inside that same expression, proving defs stay visible. Sum is 21.
    assert_eq!(
        eval_with_stdlib("(def n 5)\n(def n 20)\n(+ n (sys/join (sys/spawn-vm (fn () 1))))"),
        "21",
        "expression runs must still execute under ev/run (so scheduler-dependent \
         sys/join works) while top-level defs remain visible to them"
    );
}

// ── The backend-tier region gauge pins ───────────────────────────────
//
// docs/impl/region/diagnostics.md § "The backend-tier gauge". The arena gauges
// are host-side and tier-transparent, so a program sampling them under the
// full-module WASM tier measures the tier's own region reclamation. Every
// region instruction is a structural no-op in this emitter, so an ALLOCATING
// boundary call strands its fresh region to process teardown — the pinned
// program-duration over-keep. The pins are shrink-only: realizing region
// release on this tier lowers them toward the VM's zero; they must never rise.

/// Run stdlib-free source through the full-module WASM backend and return the
/// result's display form (materialized while the heap is alive; see
/// `eval_with_stdlib`).
fn eval(source: &str) -> String {
    match super::eval_wasm(source, "<gauge>") {
        Ok(s) => s,
        Err(e) => panic!("eval_wasm failed: {}", e),
    }
}

#[test]
fn wasm_gauge_is_live() {
    // Gauge-live discriminator: a retained list MUST register on the object
    // gauge on every tier — a 0 here means the gauge is dead and every other
    // pin in this block is void, so fail loudly instead of lying.
    let grew = eval(
        "(def o0 (arena/count))\n\
         (def @acc 0)\n\
         (def @k 0)\n\
         (while (%lt k 50)\n\
           (assign acc (%pair k acc))\n\
           (assign k (%add k 1)))\n\
         (%sub (arena/count) o0)",
    );
    let grew: i64 = grew.parse().expect("gauge delta is an int");
    assert!(
        grew >= 50,
        "50 retained pairs grew the object gauge by {grew} — the gauge is dead"
    );
}

#[test]
fn wasm_full_strands_one_region_per_allocating_host_call() {
    // 200 discarded `(pair i i)` — on the VM this reclaims to 0/op; on the
    // full-module WASM tier each allocating boundary call strands its fresh
    // region. Shrink-only pin at 1 region/op.
    let grew = eval(
        "(def r0 (arena/region-count))\n\
         (def @j 0)\n\
         (while (%lt j 200)\n\
           (%pair j j)\n\
           (assign j (%add j 1)))\n\
         (%sub (arena/region-count) r0)",
    );
    let grew: i64 = grew.parse().expect("gauge delta is an int");
    eprintln!("[gauge] wasm-full strand: {grew} regions / 200 allocating ops");
    assert!(
        grew <= 200,
        "200 discarded pairs stranded {grew} regions — the WASM tier's \
         over-keep grew past 1 region per allocating host call"
    );
    assert!(
        grew == 0 || grew == 200,
        "200 discarded pairs stranded {grew} regions — neither the pinned \
         over-keep (200) nor full reclamation (0); re-measure and re-pin"
    );
}

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

#[test]
fn wasm_full_non_allocating_calls_strand_nothing() {
    // The per-boundary-call region MINT is free: entries materialize lazily on
    // first allocation (regionstore/alloc.rs), so a pure-compute loop must not
    // move the region gauge at all.
    let grew = eval(
        "(def r0 (arena/region-count))\n\
         (def @j 0)\n\
         (while (%lt j 200)\n\
           (assign j (%add j 1)))\n\
         (%sub (arena/region-count) r0)",
    );
    let grew: i64 = grew.parse().expect("gauge delta is an int");
    assert_eq!(
        grew, 0,
        "a non-allocating loop stranded {grew} regions — an unused boundary \
         mint is materializing region entries"
    );
}
