//! audited: 2026-09-16
//! What the pending table's tests build on: an entry that owns nothing, an
//! entry that holds one value, and a value in a region of its own.

use super::*;
use crate::io::pool::BufferPool;
use crate::value::fiber::FiberStatus;
use crate::value::heap::HeapObject;
use crate::value::Value;

/// A `Sleep` entry: the one variant that owns nothing but its buffer, so a
/// table test can file and retire it without a port, a child or a socket.
fn sleep_op(pool: &mut BufferPool) -> PendingOp {
    PendingOp::Sleep {
        buffer_handle: pool.alloc(0),
    }
}

fn id(n: u64) -> SubmissionId {
    SubmissionId::from_raw(n)
}

/// A submission with neither a store nor a fiber: a `Sleep` holds no values
/// to retain, and no fiber means nothing is ever withheld or swept.
fn detached() -> Submitter {
    Submitter::detached(std::ptr::null_mut())
}

/// An entry holding one heap value. `WatchNext` is the shape with a single
/// operand and nothing else to stand up.
fn watch_op(pool: &mut BufferPool, watcher: Value) -> PendingOp {
    PendingOp::WatchNext {
        watcher,
        buffer_handle: pool.alloc(0),
    }
}

use crate::value::fiber::test_fiber_in_region as fiber_in;

/// A heap, a region on it, and a value born in that region.
fn value_in_fresh_region() -> (
    *mut crate::value::fiberheap::FiberHeap,
    crate::hir::region::RuntimeRegion,
    Value,
) {
    let heap = crate::value::arena::leaked_test_heap();
    // SAFETY: the heap is leaked for the process.
    let h = unsafe { &mut *heap };
    let region = h.new_runtime_region();
    let value = h.alloc_in_region(
        HeapObject::LBox {
            cell: std::rc::Rc::new(std::cell::RefCell::new(Value::NIL)),
            traits: Value::NIL,
        },
        region,
    );
    (heap, region, value)
}

mod hold;
mod take;
