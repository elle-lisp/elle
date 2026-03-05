//! Custom allocator primitives for `with-allocator`.
//!
//! `%install-allocator` and `%uninstall-allocator` are internal primitives
//! that push/pop custom allocators on the current fiber's `FiberHeap`.
//!
//! # Safety
//!
//! These primitives must only be used via the `with-allocator` prelude macro.
//! The `ArenaMark.custom_ptrs_len` field records the position in the *current*
//! (innermost) custom allocator's `custom_ptrs` at `RegionEnter` time. This is
//! safe because `with-allocator` desugars to `defer`, which wraps the body in
//! a fiber — the body's scope marks live on the child fiber's `FiberHeap`,
//! separate from the parent's. If anyone calls `%install-allocator`/
//! `%uninstall-allocator` directly without a fiber boundary between install
//! and scope marks, `RegionExit` may dealloc from a popped allocator
//! (use-after-free). **These primitives must only be used via the
//! `with-allocator` macro.**

use std::rc::Rc;

use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::allocator::AllocatorBox;
use crate::value::fiber::SignalBits;
use crate::value::fiber::{SIG_ERROR, SIG_OK};
use crate::value::fiber_heap::with_current_heap_mut;
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// (%install-allocator allocator-value)
///
/// Takes one argument: an ExternalObject wrapping an `AllocatorBox`.
/// Extracts the `Rc<AllocatorBox>` and pushes a `CustomAllocState` onto
/// the current fiber's `FiberHeap`.
///
/// # Safety
///
/// Must only be called via the `with-allocator` macro. See module doc.
fn prim_install_allocator(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "%install-allocator: expected 1 argument, got {}",
                    args.len()
                ),
            ),
        );
    }

    // Extract Rc<AllocatorBox> from the ExternalObject.
    // The ExternalObject.data is Rc<dyn Any>. We need to get Rc<AllocatorBox>.
    // value.as_external::<AllocatorBox>() gives &AllocatorBox (a ref into the Rc).
    // We need the Rc itself to clone it into CustomAllocState.
    //
    // Since ExternalObject.data is Rc<dyn Any>, and the concrete type is
    // AllocatorBox, we can access it via the heap object directly.
    let alloc_box: Rc<AllocatorBox> = match extract_allocator_rc(args[0]) {
        Some(rc) => rc,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "%install-allocator: expected an allocator (ExternalObject \
                         wrapping AllocatorBox), got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };

    match with_current_heap_mut(|heap| {
        heap.push_custom_allocator(alloc_box);
    }) {
        Some(()) => (SIG_OK, Value::NIL),
        None => (
            SIG_ERROR,
            error_val(
                "state-error",
                "%install-allocator: no fiber heap installed (root fiber?)".to_string(),
            ),
        ),
    }
}

/// (%uninstall-allocator)
///
/// Takes no arguments. Pops the top custom allocator from the current fiber's
/// `FiberHeap`, runs Drop for remaining custom objects, then calls dealloc
/// for each.
///
/// # Safety
///
/// Must only be called via the `with-allocator` macro. See module doc.
fn prim_uninstall_allocator(args: &[Value]) -> (SignalBits, Value) {
    if !args.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "%uninstall-allocator: expected 0 arguments, got {}",
                    args.len()
                ),
            ),
        );
    }

    match with_current_heap_mut(|heap| heap.pop_custom_allocator()) {
        Some(true) => (SIG_OK, Value::NIL),
        Some(false) => (
            SIG_ERROR,
            error_val(
                "state-error",
                "%uninstall-allocator: no custom allocator installed".to_string(),
            ),
        ),
        None => (
            SIG_ERROR,
            error_val(
                "state-error",
                "%uninstall-allocator: no fiber heap installed (root fiber?)".to_string(),
            ),
        ),
    }
}

/// Extract `Rc<AllocatorBox>` from a Value that is an ExternalObject.
///
/// The ExternalObject stores `Rc<dyn Any>`. We downcast to `AllocatorBox`
/// and clone the `Rc` (cheap — just a refcount bump).
fn extract_allocator_rc(value: Value) -> Option<Rc<AllocatorBox>> {
    use crate::value::heap::{deref, HeapObject};
    if !value.is_heap() {
        return None;
    }
    unsafe {
        match deref(value) {
            HeapObject::External(ext) => {
                // Try to downcast Rc<dyn Any> to Rc<AllocatorBox>.
                // Rc::downcast is available for Rc<dyn Any>.
                let rc_any: Rc<dyn std::any::Any> = ext.data.clone();
                rc_any.downcast::<AllocatorBox>().ok()
            }
            _ => None,
        }
    }
}

pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "%install-allocator",
        func: prim_install_allocator,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Install a custom allocator on the current fiber's heap. \
              INTERNAL: use via with-allocator macro only.",
        params: &["allocator"],
        category: "allocator",
        example: "",
        aliases: &[],
    },
    PrimitiveDef {
        name: "%uninstall-allocator",
        func: prim_uninstall_allocator,
        effect: Effect::none(),
        arity: Arity::Exact(0),
        doc: "Uninstall the current custom allocator, freeing remaining \
              custom objects. INTERNAL: use via with-allocator macro only.",
        params: &[],
        category: "allocator",
        example: "",
        aliases: &[],
    },
];
