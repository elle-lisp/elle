// audited: 2026-10-06
//! Unit tests of a runtime's lifecycle, its root entry and the regions its programs own.
//!
//! src/runtime.rs

use super::*;
use crate::value::arena::{alloc_in_fresh_region, region_rc, register_process_root_region};
use crate::value::heap::{HeapObject, Pair};

fn cons() -> HeapObject {
    HeapObject::Pair(Pair::new(
        crate::value::Value::NIL,
        crate::value::Value::NIL,
    ))
}

mod heaps;
mod lifecycle;
mod operandstack;
mod ownership;
mod rootentry;
mod selfrec;
#[cfg(feature = "mlir")]
mod spirv;
