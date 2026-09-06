// audited: 2026-09-06
// docs/impl/wasm.md
// docs/impl/region/template.md
//! The code object `rt_make_closure` builds for a WASM closure.
//!
//! The module carries one dual-compiled blueprint per closure, and the host
//! function builds the code object from it plus the shape the compiled code
//! passes through linear memory. A spawned OS-thread VM worker runs that code
//! object's bytecode, so every field of the blueprint is read there: the
//! nested-lambda blueprints a `MakeClosure` indexes, and the release tables an
//! abandoned frame walks.

use super::*;
use crate::syntax::Span;
use crate::value::closure::Closure;

/// Where the nested lambda's one instruction is written. The fixture's spans
/// are synthetic, and the emitter records a location only for a real one, so a
/// test whose subject is the location map writes its own.
const FILE: &str = "wasm-closure-blueprint.lisp";
const LINE: u32 = 12;
const COL: u32 = 5;

/// A nullary lambda carrying one of everything the dual-compiled blueprint
/// copies off its own emission: a source location, a merge set, and both
/// release tables.
fn nested_lambda_lir() -> LirFunction {
    let mut func = LirFixture::new(Arity::Exact(0))
        .closure_id(ClosureId(0))
        .name("nested")
        .signal(Signal::silent())
        .block(
            0,
            vec![LirInstr::Const {
                dst: Reg(0),
                value: LirConst::Nil,
            }],
            Terminator::Return(Reg(0)),
        )
        .build();
    func.blocks[0].instructions[0].span = Span::new(0, 3, LINE, COL).with_file(FILE);
    func.merged_slots = vec![static_region(5)];
    func.frame_release_slots = vec![3, 7];
    func.frame_release_regions = vec![static_region(11), static_region(13)];
    func
}

/// A nullary entry whose whole body is one `MakeClosure` of closure 0, so the
/// value the module returns is the closure that `MakeClosure` built.
fn entry_making_closure() -> LirFunction {
    LirFixture::new(Arity::Exact(0))
        .signal(Signal::silent())
        .block(
            0,
            vec![LirInstr::MakeClosure {
                dst: Reg(0),
                closure_id: ClosureId(0),
                captures: vec![],
                region: static_region(2),
            }],
            Terminator::Return(Reg(0)),
        )
        .build()
}

/// Run a module of `nested_lambda_lir` under `entry_making_closure` through the
/// whole full-module path, and hand the closure it returns to `read`.
///
/// The closure and its code object are allocations of the driving VM's heap, so
/// `read` runs while that VM and the wasmtime store are both still alive — the
/// value would dangle after either goes.
fn with_entry_closure<T>(read: impl FnOnce(&Closure) -> T) -> T {
    let mut vm = crate::vm::VM::new();
    let module = LirModule {
        entry: entry_making_closure(),
        closures: vec![nested_lambda_lir()],
    };
    let emitted = emit_module(
        &module,
        std::collections::HashSet::new(),
        vm.heap_ptr,
        std::ptr::null_mut(),
    );
    let engine = super::super::store::create_engine().expect("a wasmtime engine");
    let mut store = super::super::store::create_store(
        &engine,
        emitted.const_pool,
        emitted.closure_bytecodes,
        emitted.env_stack_base,
    );
    store.data_mut().vm = &mut vm as *mut crate::vm::VM;
    let linker = super::super::linker::create_linker(&engine).expect("a linker");
    let compiled =
        super::super::store::compile_module(&engine, &emitted.wasm_bytes).expect("a module");
    let value = super::super::store::run_module(&linker, &mut store, &compiled)
        .expect("the module runs to completion");
    let closure = value
        .as_closure()
        .expect("the entry returns the closure its MakeClosure built");
    read(closure)
}

#[test]
fn a_wasm_built_closure_carries_the_frame_release_tables() {
    // Counter-factual: leaving both tables to the empty value
    // `TemplateProto::new` supplies fails nothing that runs. The closure carries
    // real bytecode and returns the right answers; what it loses is one error
    // exit's walk on the spawned worker, which strands every region the
    // abandoned frame still owed.
    let (slots, regions) = with_entry_closure(|closure| {
        (
            closure.template.frame_release_slots().to_vec(),
            closure.template.frame_release_regions().to_vec(),
        )
    });
    assert_eq!(
        slots,
        vec![3u16, 7],
        "the value route's slots reach the code object",
    );
    assert_eq!(
        regions,
        vec![11u32, 13],
        "the slot route's regions reach the code object",
    );
}

#[test]
fn a_wasm_built_closure_carries_the_locations_and_the_merge_set_its_body_names() {
    // The other two fields the dual-compiled blueprint carries, and the same
    // silence: a merge set left empty mints a fresh region where the body meant
    // to reuse one, and a location map left empty reports an error against no
    // source line at all.
    let (merged, location) = with_entry_closure(|closure| {
        (
            closure.template.merged_slots().as_slice().to_vec(),
            closure.template.locations().first(),
        )
    });
    assert_eq!(merged, vec![5u32], "the merge set reaches the code object");
    let location = location.expect("the body's one instruction has a location");
    assert_eq!(
        (location.file.as_str(), location.line, location.col),
        (FILE, LINE as usize, COL as usize),
        "the source location of the body reaches the code object",
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
