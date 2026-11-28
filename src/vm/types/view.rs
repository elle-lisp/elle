// audited: 2026-11-24
//! The view `match` reads a syntax object through: one level of the node as data, built in the node's own region.
//!
//! docs/macros/syntax.md

use crate::primitives::ctx::NativeCtx;
use crate::value::Value;
use crate::vm::core::VM;

/// What `match` reads `value` as: the view of a syntax object, and any other
/// value unchanged.
///
/// The view is built in the region that holds the node, so a name bound
/// through it borrows the scrutinee as an element read does, and the view
/// needs no release of its own. Each child is a header over the tree that
/// region already holds.
///
/// A node with no view, a struct literal whose key no struct holds, reads as
/// itself, so no compound pattern takes it. The step never signals: every
/// match takes it, and a signal there would leave a function that signal
/// inference proved silent.
pub(crate) fn syntax_view(vm: &mut VM, value: Value) -> Value {
    let Some(node) = value.as_syntax().copied() else {
        return value;
    };
    let vm_ptr: *mut VM = vm;
    let heap = unsafe { &mut *vm.heap_ptr };
    let Some(region) = heap.owning_region_of(value) else {
        debug_assert!(
            false,
            "syntax_view: a syntax object no region of this heap holds"
        );
        return value;
    };
    debug_assert!(!vm.symbols_ptr.is_null(), "syntax_view: no symbol table");
    let Some(symbols) = (unsafe { vm.symbols_ptr.as_mut() }) else {
        return value;
    };
    let mut ctx = NativeCtx::with_region_vm(region, heap, vm_ptr);
    node.view(symbols, &mut ctx, |alloc, child| {
        alloc.syntax_resident(child)
    })
    .unwrap_or(value)
}

pub(crate) fn handle_syntax_view(vm: &mut VM) {
    let value = vm
        .fiber
        .stack
        .pop()
        .expect("VM bug: Stack underflow on SyntaxView");
    let view = syntax_view(vm, value);
    vm.fiber.stack.push(view);
}
