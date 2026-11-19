// audited: 2026-11-18
//! The helper compiled code calls for a `MakeClosure`: the interpreter's own builder, over a header the code holds as a constant.
//!
//! docs/impl/jit-closure.md

use super::region_of_raw;
use crate::jit::value::JitValue;
use crate::value::closure::TemplateRef;
use crate::value::Value;

/// Build a closure over the child header `(child_tag, child_payload)` and the
/// `count` captures at `captures`, in the region `region` names.
///
/// The translator baked the header's two words into the call; they name a
/// `ClosureTemplate` in the parent's code region, which the cache entry's pin
/// holds while this code is installed. The build is the interpreter's
/// `materialize_closure_in_region`, so both tiers make the same three objects
/// in the same region: a fresh header, the environment, and the instance.
#[no_mangle]
pub extern "C" fn elle_jit_make_closure(
    captures: *const Value,
    count: u64,
    child_tag: u64,
    child_payload: u64,
    region: u32,
    vm: *mut (),
) -> JitValue {
    let captures: &[Value] = if count == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(captures, count as usize) }
    };
    let child = TemplateRef::region(Value {
        tag: child_tag,
        payload: child_payload,
    });
    // The driving VM's own heap, via the threaded vm pointer (docs/impl/region/ctx.md).
    let heap = unsafe { &mut *(*(vm as *mut crate::vm::VM)).heap_ptr };
    JitValue::from_value(crate::vm::closure::materialize_closure_in_region(
        heap,
        &child,
        captures,
        region_of_raw(region),
    ))
}
