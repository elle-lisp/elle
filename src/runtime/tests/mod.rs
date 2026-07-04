//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::config::region_ownership_override::{RegionOwnership, ScopedRegionOwnership};
use crate::value::arena::{alloc_in_fresh_region, region_rc, register_process_root_region};
use crate::value::heap::{HeapObject, Pair};

fn cons() -> HeapObject {
    HeapObject::Pair(Pair::new(
        crate::value::Value::NIL,
        crate::value::Value::NIL,
    ))
}

mod lifecycle;
mod ownership;
mod selfrec;
