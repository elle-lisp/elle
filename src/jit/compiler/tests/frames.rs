// audited: 2026-11-23
// docs/impl/jit-map.md
//! Every function the JIT compiles on x86_64 opens a frame, which is what lets
//! perf's frame-pointer unwinder walk through it.

use super::*;

/// `push %rbp; mov %rsp, %rbp`, the frame setup Cranelift's x86_64 backend
/// emits.
#[cfg(target_arch = "x86_64")]
const FRAME_SETUP: [u8; 4] = [0x55, 0x48, 0x89, 0xe5];

/// The counter-factual: perf's DWARF unwinder stops at a JIT frame, so the
/// frame pointer is the only link from a sampled JIT function to its caller.
/// An entry without this setup would cut every such stack there, and no other
/// test would fail.
#[cfg(target_arch = "x86_64")]
#[test]
fn a_compiled_function_opens_a_frame() {
    let lir = simple_fixture(Arity::Exact(1)).build();
    let code = verified()
        .compile(&lir.view(), crate::jit::ChildTable::EMPTY)
        .expect("Failed to compile");
    // SAFETY: the code is finalized and lives as long as `code`, and every
    // x86_64 function is longer than its frame setup.
    let entry = unsafe { std::slice::from_raw_parts(code.fn_ptr(), FRAME_SETUP.len()) };
    assert_eq!(entry, FRAME_SETUP, "the entry must open a frame");
}
