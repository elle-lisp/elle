// audited: 2026-09-29
//! The JIT runtime helpers' pins, one submodule per subject, and the VM set-up they share.
//!
//! docs/impl/jit.md
//! docs/impl/region/owner.md
//!
//! - `push` — the push helpers mutate an @array in place and count what they insert.
//! - `copies` — the copying helpers allocate into the region they are handed.
//! - `adopt` — the adopt and group-free helpers resolve Values and forward.
//! - `exits` — the pending-exception probe and the compiled error exit.

use super::*;
use crate::hir::region::RuntimeRegion;
use crate::jit::value::JitValue;
use crate::primitives::ctx::NativeCtx;
use crate::vm::VM;

mod adopt;
mod copies;
mod exits;
mod push;

/// A VM with the primitives registered, as the helpers that call back into them
/// need.
fn vm_with_primitives() -> VM {
    let mut symbols = crate::symbol::SymbolTable::new();
    let mut vm = VM::new();
    let _signals = crate::primitives::register_primitives(&mut vm, &mut symbols);
    vm
}

/// An allocation context over `vm`'s own heap that places what it builds in
/// `region`. The helpers reach the heap through the VM, so a test value has to
/// live there, and a test that asserts where a result lands names the source
/// region explicitly.
fn ctx_in(vm: &mut VM, region: RuntimeRegion) -> NativeCtx<'_> {
    let heap_ptr = vm.heap_ptr;
    NativeCtx::with_region_vm(region, unsafe { &mut *heap_ptr }, vm as *mut VM)
}

/// The region `value` lives in, on `vm`'s heap.
fn region_of(vm: &VM, value: Value) -> Option<RuntimeRegion> {
    crate::value::arena::region_of(unsafe { &mut *vm.heap_ptr }, value)
}

/// `vm` as the untyped pointer the helpers take across the C ABI.
fn vm_arg(vm: &mut VM) -> *mut () {
    vm as *mut VM as *mut ()
}
