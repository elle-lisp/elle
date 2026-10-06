// audited: 2026-10-06
//! A JIT cache entry, and a compile in flight, each pin the code region their key's payload lives in.
//!
//! docs/impl/jit.md
//!
//! `jit_cache` and `jit_pending` key entries by the raw address of a code
//! object's bytecode. Without the pin, a dropped unit frees its code region, a
//! later unit lands a different function's bytecode at the same address, and
//! the cache hands the new function the old function's compiled code — wrong
//! body, new env and args, no memory corruption to observe.

use std::sync::Arc;

use crate::jit::JitCode;
use crate::value::{Arity, ClosureTemplate, CodeBuilder};
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
    let (unit, template) = probe(&mut vm, 7, 371);
    let (id, generation) = code_region(&mut vm, &template);

    vm.install_jit_code(
        template.clone(),
        Arc::new(JitCode::test_with_yield_points(Vec::new())),
    );
    drop(unit);
    drop(template);
    assert_eq!(
        vm.heap().region_generation(id),
        generation,
        "the unit is gone, but the cache entry keyed by its bytecode still pins \
         the code region"
    );

    vm.jit_cache.clear();
    assert_ne!(
        vm.heap().region_generation(id),
        generation,
        "with the entry gone nothing holds the code region, so it frees"
    );
}

/// Same invariant for the in-flight window: a compile submitted to the
/// background worker keys its future cache entry by address, so the pin must
/// start at submission, not at install.
#[test]
fn a_pending_compile_pins_its_code_region() {
    let mut vm = VM::new();
    let (unit, template) = probe(&mut vm, 9, 373);
    let (id, generation) = code_region(&mut vm, &template);

    vm.record_jit_pending(template.clone());
    drop(unit);
    drop(template);
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
