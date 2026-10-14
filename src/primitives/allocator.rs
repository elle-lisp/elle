// audited: 2026-10-14
//! The `with-allocator` primitives: push a custom allocator on the instance's heap, and pop it.
//!
//! docs/impl/values.md
//!
//! The prelude's `with-allocator` installs the allocator, runs its body under
//! `defer`, and uninstalls it even when the body raises. Popping runs the
//! destructors the allocator's state records and frees the memory it records.
//! No allocation routes through a pushed allocator, so that state stays empty.

use crate::primitives::def::RegionEffect;
use std::rc::Rc;

use crate::signals::Signal;
use crate::value::allocator::AllocatorBox;
use crate::value::fiber::SignalBits;
use crate::value::fiber::{SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

/// (allocator/install allocator-value)
///
/// Takes one argument: an ExternalObject wrapping an `AllocatorBox`.
/// Extracts the `Rc<AllocatorBox>` and pushes a `CustomAllocState` onto
/// the instance's heap.
fn prim_install_allocator(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
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
            return type_error!(
                ctx,
                args[0],
                "allocator/install",
                "an allocator (ExternalObject \
                         wrapping AllocatorBox)"
            );
        }
    };

    ctx.heap_mut().push_custom_allocator(alloc_box);
    (SIG_OK, Value::NIL)
}

/// (allocator/uninstall)
///
/// Takes no arguments. Pops the top custom allocator from the instance's
/// heap, runs Drop for remaining custom objects, then calls dealloc for each.
fn prim_uninstall_allocator(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    if ctx.heap_mut().pop_custom_allocator() {
        (SIG_OK, Value::NIL)
    } else {
        (
            SIG_ERROR,
            ctx.error(
                "state-error",
                "allocator/uninstall: no custom allocator installed".to_string(),
            ),
        )
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
            HeapObject::External { obj: ext, .. } => {
                // Try to downcast Rc<dyn Any> to Rc<AllocatorBox>.
                // Rc::downcast is available for Rc<dyn Any>.
                let rc_any: Rc<dyn std::any::Any> = ext.data.clone();
                rc_any.downcast::<AllocatorBox>().ok()
            }
            _ => None,
        }
    }
}

primitive! {
    "allocator/install" => prim_install_allocator {
        ambient: true,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Install a custom allocator on the instance's heap. \
               INTERNAL: use via with-allocator macro only.",
        params: &["allocator"],
        category: "allocator",
        effect: RegionEffect::Immediate,
    }
    "allocator/uninstall" => prim_uninstall_allocator {
        ambient: true,
        signal: Signal::errors(),
        doc: "Uninstall the current custom allocator, freeing remaining \
               custom objects. INTERNAL: use via with-allocator macro only.",
        category: "allocator",
        effect: RegionEffect::Immediate,
    }
}
