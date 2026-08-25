//! Cache-identity tests for the JIT entry points.
//!
//! `jit_cache` keys entries by the raw address of a template's bytecode
//! allocation. The invariant under test (docs/impl/jit.md § "Cache
//! identity"): an entry pins the bytecode allocation it is keyed by, so the
//! address cannot be freed and reused by a different function while the
//! entry lives. Without the pin, a template drop plus an address-reusing
//! allocation hands the new function the old function's compiled code —
//! wrong body, new env and args, no memory corruption to observe.

use std::rc::Rc;
use std::sync::Arc;

use crate::jit::JitCode;
use crate::vm::VM;

/// Distinctive allocation size for the probe: any size works, but an odd
/// one keeps unrelated runtime allocations from landing in the same
/// size class mid-test.
const PROBE_LEN: usize = 371;

/// Try to get the allocator to hand back `target` for a fresh allocation of
/// the same size. Failed candidates are kept alive so the allocator must
/// keep producing new addresses instead of recycling the same wrong one.
/// Returns true if `target` was reissued.
fn address_reissued(target: *const u8, tries: usize) -> bool {
    let mut hold = Vec::with_capacity(tries);
    for _ in 0..tries {
        let candidate: Vec<u8> = vec![9u8; PROBE_LEN];
        if candidate.as_ptr() == target {
            return true;
        }
        hold.push(candidate);
    }
    false
}

/// The wrong answer that looked right: keying by address is fine as long as
/// the allocation is immortal — and templates USED to be process-lifetime
/// data, so nothing ever checked. Templates are region-reclaimed now; a live
/// cache entry must therefore keep its keyed allocation alive itself.
#[test]
fn jit_cache_entry_pins_its_keyed_bytecode_allocation() {
    let mut vm = VM::new();
    let bytecode: Rc<Vec<u8>> = Rc::new(vec![7u8; PROBE_LEN]);
    let ptr = bytecode.as_ptr();

    vm.install_jit_code(
        bytecode.clone(),
        Arc::new(JitCode::test_with_yield_points(Vec::new())),
    );

    // The template drops; the cache entry survives.
    drop(bytecode);

    assert!(
        !address_reissued(ptr, 10_000),
        "jit_cache key address was reallocated while its entry is live: \
         a new function's bytecode at this address would dispatch the old \
         function's compiled code"
    );
}

/// Same invariant for the in-flight window: a compile submitted to the
/// background worker keys its future cache entry by address, so the pin
/// must start at submission, not at install.
#[test]
fn jit_pending_entry_pins_its_keyed_bytecode_allocation() {
    let mut vm = VM::new();
    let bytecode: Rc<Vec<u8>> = Rc::new(vec![7u8; PROBE_LEN]);
    let ptr = bytecode.as_ptr();

    vm.record_jit_pending(bytecode.clone());

    drop(bytecode);

    assert!(
        !address_reissued(ptr, 10_000),
        "jit_pending key address was reallocated while its compile is in \
         flight: the worker's result would install under an address now \
         owned by a different function"
    );
}
