// audited: 2026-11-24
//! A cache entry pins its code and keeps the first, a compile in flight pins too, a count dies with its function, a settle leaves none.
//!
//! docs/impl/jit.md
//!
//! `jit_cache` and `jit_pending` key entries by the raw address of a code
//! object's bytecode. Without the pin, a dropped unit frees its code region, a
//! later unit lands a different function's bytecode at the same address, and
//! the cache hands the new function the old function's compiled code — wrong
//! body, new env and args, no memory corruption to observe.
//!
//! The tests of a tail call that reaches a callee's compiled code are in
//! `tail.rs`.

use std::sync::Arc;

use crate::jit::{JitCode, JitValue};
use crate::value::{Arity, ClosureTemplate, CodeBuilder, Value};
use crate::vm::VM;

/// A unit on `vm`'s heap whose entry runs `len` bytes of `fill`, and its
/// entry's code object.
fn probe(vm: &mut VM, fill: u8, len: usize) -> (crate::value::CodeUnit, ClosureTemplate) {
    let unit = CodeBuilder::new(vec![fill; len], Arity::Exact(0), Vec::new()).unit(vm.heap());
    let template = unit.entry().clone();
    (unit, template)
}

/// The id and generation of the region `t`'s payload lives in.
fn code_region(vm: &mut VM, t: &ClosureTemplate) -> (u32, u32) {
    let id = vm.heap().region_of_ptr(t.payload_backing());
    (id, vm.heap().region_generation(id))
}

/// The wrong answer that looked right: keying by address is fine as long as
/// the allocation is immortal — and templates used to be process-lifetime
/// data, so nothing checked. A code region frees once its unit and its last
/// header are gone, so a live cache entry must hold the region itself.
///
/// The second half is the counter-factual for the first: with the entry gone,
/// nothing else holds the region, so the generation the first half compared
/// against really would have moved.
#[test]
fn a_jit_cache_entry_pins_its_code_region() {
    let mut vm = VM::new();
    // The unit and every handle on its entry go out of scope at the brace.
    let (id, generation) = {
        let (_unit, template) = probe(&mut vm, 7, 371);
        let region = code_region(&mut vm, &template);
        vm.install_jit_code(
            template,
            Arc::new(JitCode::test_with_yield_points(Vec::new())),
        );
        region
    };
    assert_eq!(
        vm.heap().region_generation(id),
        generation,
        "the unit is gone, but the cache entry keyed by its bytecode still pins \
         the code region"
    );

    vm.clear_code_pins();
    assert_ne!(
        vm.heap().region_generation(id),
        generation,
        "with the entry gone nothing holds the code region, so it frees"
    );
}

/// Code that answers 1, standing in for a compile of a function.
extern "C" fn answers_1(
    _env: *const Value,
    _args: *const Value,
    _nargs: u32,
    _vm: *mut (),
    _self_tag: u64,
    _self_payload: u64,
) -> JitValue {
    JitValue::from_value(Value::int(1))
}

/// Code that answers 2, standing in for a second compile of the same function.
extern "C" fn answers_2(
    _env: *const Value,
    _args: *const Value,
    _nargs: u32,
    _vm: *mut (),
    _self_tag: u64,
    _self_payload: u64,
) -> JitValue {
    JitValue::from_value(Value::int(2))
}

/// An install over a key that holds code keeps that code, so the code a call
/// borrowed stays alive while it runs (docs/impl/jit.md). A forced compile and
/// the worker's compile of the same function are the two installs.
///
/// The counter-factual is an install that replaces the entry: the cache then
/// answers the second code, and frees the first under any call still in it.
#[test]
fn an_install_over_a_live_key_keeps_the_first_code() {
    let mut vm = VM::new();
    let (_unit, template) = probe(&mut vm, 19, 377);
    vm.install_jit_code(
        template.clone(),
        Arc::new(JitCode::test_with_entry(answers_1)),
    );
    vm.install_jit_code(
        template.clone(),
        Arc::new(JitCode::test_with_entry(answers_2)),
    );
    let code = vm
        .jit_code_for(template.bytecode().as_ptr())
        .expect("the function has compiled code");
    assert_eq!(
        code.fn_ptr(),
        answers_1 as *const u8,
        "the first install's code stays in the cache"
    );
}

/// Same invariant for the in-flight window: a compile submitted to the
/// background worker keys its future cache entry by address, so the pin must
/// start at submission, not at install.
#[test]
fn a_pending_compile_pins_its_code_region() {
    let mut vm = VM::new();
    let (id, generation) = {
        let (_unit, template) = probe(&mut vm, 9, 373);
        let region = code_region(&mut vm, &template);
        vm.record_jit_pending(template);
        region
    };
    assert_eq!(
        vm.heap().region_generation(id),
        generation,
        "the compile in flight still pins the code region its key names"
    );

    vm.jit_pending.clear();
    assert_ne!(
        vm.heap().region_generation(id),
        generation,
        "with the compile settled nothing holds the code region, so it frees"
    );
}

/// A call count takes no pin, so a freed function's count must not survive
/// its function (docs/impl/jit.md).
///
/// The counter-factual is a count keyed by address alone: it answers 3 for an
/// address whose function is gone, and the next function to land there starts
/// three calls closer to hot than it has run.
#[test]
fn a_freed_functions_call_count_reads_as_zero() {
    let mut vm = VM::new();
    let key = {
        let (_unit, template) = probe(&mut vm, 11, 375);
        for _ in 0..3 {
            vm.record_closure_call(&template);
        }
        let key = template.bytecode().as_ptr();
        assert_eq!(vm.closure_call_count(key), 3, "three calls, counted");
        key
    };
    assert_eq!(
        vm.closure_call_count(key),
        0,
        "the unit and every header are gone, so the count names no function"
    );
}

/// Counts of freed functions leave the table as it grows, so a program that
/// compiles in a loop does not keep one count per function it ever called.
///
/// The second set is built while the first is alive, so no address of the
/// second names a function of the first, and nothing but the sweep can drop
/// the first set's counts. The counter-factual is a table that never drops
/// one: it ends holding both sets.
#[test]
fn dead_call_counts_leave_the_table() {
    const N: usize = 2000;
    let mut vm = VM::new();
    let first: Vec<_> = (0..N).map(|_| probe(&mut vm, 13, 64)).collect();
    for (_, template) in &first {
        vm.record_closure_call(template);
    }
    let second: Vec<_> = (0..N).map(|_| probe(&mut vm, 17, 64)).collect();
    drop(first);
    for (_, template) in &second {
        vm.record_closure_call(template);
    }
    assert!(
        vm.closure_call_counts.len() <= N,
        "only the live set's counts remain, not the freed set's: the table holds {}",
        vm.closure_call_counts.len()
    );
    drop(second);
}

/// `--dump=stats` settles the queue rather than drain it (docs/config.md).
/// Each pending compile is either taken, and counted, or dropped, so nothing
/// stays pending and nothing counts twice. The counter-factual is the drain:
/// it drops nothing, and waits for every one of four hundred compiles.
#[test]
fn settling_the_queue_takes_or_drops_each_pending_compile() {
    use crate::config::JitPolicy;
    use crate::pipeline::compile_file_repl;
    let mut rt = crate::runtime::Runtime::new();
    rt.vm().runtime_config.jit = JitPolicy::Eager;
    let fns: String = (0..400).map(|i| format!("(fn [x] (+ x {i})) ")).collect();
    let src = format!("(def fs [{fns}]) (each f in fs (f 1)) nil");
    let prog = {
        let (_vm, symbols, cctx) = rt.parts();
        compile_file_repl(&src, symbols, cctx, "<settle>")
            .expect("compiles")
            .0
    };
    {
        let (vm, _symbols, cctx) = rt.parts();
        vm.execute_scheduled(&prog, cctx)
            .expect("runs (submits the JIT tasks)");
    }
    let vm = rt.vm();
    let pending = vm.jit_pending.len() as u64;
    let before = vm.jit_stats.compiles();
    let dropped = vm.settle_jit_queue() as u64;
    assert!(
        vm.jit_pending.is_empty(),
        "{} compiles stay pending after the settle",
        vm.jit_pending.len()
    );
    assert!(
        dropped > 0,
        "the settle dropped none of {pending} pending compiles, queued faster than the thread compiles"
    );
    assert_eq!(
        vm.jit_stats.compiles() - before + dropped,
        pending,
        "each pending compile is taken or dropped, once"
    );
}
mod tail;
