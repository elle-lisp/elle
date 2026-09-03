//! Cache-identity tests for the JIT entry points.
//!
//! `jit_cache` keys entries by the raw address of a code object's bytecode.
//! The invariant under test (docs/impl/jit.md § "Cache identity"): an entry
//! pins the code object it is keyed by, so its payload region cannot be
//! released and its pages handed to a different function while the entry
//! lives. Without the pin, a code object drop plus an address-reusing payload
//! hands the new function the old function's compiled code — wrong body, new
//! env and args, no memory corruption to observe.

use std::rc::Rc;
use std::sync::Arc;

use crate::jit::JitCode;
use crate::value::{Arity, ClosureTemplate, TemplateProto};
use crate::vm::VM;

/// Distinctive bytecode length for the probe: any length works, but an odd one
/// keeps unrelated payloads from landing at the same offsets in a page.
const PROBE_LEN: usize = 371;

/// A code object whose bytecode is `PROBE_LEN` bytes of `fill`, materialized on
/// `vm`'s heap.
fn probe_template(vm: &mut VM, fill: u8) -> ClosureTemplate {
    let proto = Rc::new(TemplateProto::new(
        vec![fill; PROBE_LEN],
        Arity::Exact(0),
        Vec::new(),
    ));
    ClosureTemplate::for_proto(vm.heap(), &proto)
}

/// Try to get the region allocator to hand `target` back as another blueprint's
/// bytecode. Failed candidates are held alive, blueprints included, so the
/// payload cache must keep minting new addresses rather than sweeping and
/// recycling the same wrong one. Returns true if `target` was reissued.
fn payload_address_reissued(vm: &mut VM, target: *const u8, tries: usize) -> bool {
    let mut hold = Vec::with_capacity(tries);
    for i in 0..tries {
        let candidate = probe_template(vm, (i % 251) as u8);
        if candidate.bytecode().as_ptr() == target {
            return true;
        }
        hold.push(candidate);
    }
    false
}

/// The wrong answer that looked right: keying by address is fine as long as
/// the allocation is immortal — and templates USED to be process-lifetime
/// data, so nothing ever checked. A code object's payload is region-reclaimed
/// now; a live cache entry must therefore keep its keyed payload alive itself.
#[test]
fn jit_cache_entry_pins_its_keyed_bytecode_allocation() {
    let mut vm = VM::new();
    let template = probe_template(&mut vm, 7);
    let ptr = template.bytecode().as_ptr();

    vm.install_jit_code(
        template.clone(),
        Arc::new(JitCode::test_with_yield_points(Vec::new())),
    );

    // The caller's code object drops; the cache entry survives.
    drop(template);

    assert!(
        !payload_address_reissued(&mut vm, ptr, 2_000),
        "jit_cache key address was reissued while its entry is live: \
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
    let template = probe_template(&mut vm, 7);
    let ptr = template.bytecode().as_ptr();

    vm.record_jit_pending(template.clone());

    drop(template);

    assert!(
        !payload_address_reissued(&mut vm, ptr, 2_000),
        "jit_pending key address was reissued while its compile is in \
         flight: the worker's result would install under an address now \
         owned by a different function"
    );
}
