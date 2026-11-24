// audited: 2026-11-18
//! The hash of a value: its kind and contents, read breadth first, at most a fixed number of values deep.
//!
//! docs/impl/values.md
//!
//! A hash is of a value's kind and its contents, never of its heap tag, so a
//! value and its mutable twin hash alike. The hash reads the tree a value
//! unfolds into, which is the same tree for two values the order calls
//! equal, so a cycle needs no guard.

use std::collections::VecDeque;
use std::hash::{Hash, Hasher};

use super::kind::Kind;
use super::view::{with_bytes, with_elements, with_entries, with_members};
use crate::value::Value;

/// The most values one hash reads. A string or a byte sequence is one.
const BUDGET: usize = 1024;

impl Hash for Value {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut queue = VecDeque::new();
        hash_node(self, state, &mut queue, BUDGET - 1);
        let mut read = 1;
        while read < BUDGET {
            let Some(v) = queue.pop_front() else { break };
            read += 1;
            hash_node(&v, state, &mut queue, BUDGET - read);
        }
    }
}

/// Hash what `v` holds at its own place in the tree: its kind, a leaf's
/// contents, and a container's length. Queue the values a container holds,
/// in the order the container holds them, while fewer than `room` wait.
fn hash_node<H: Hasher>(v: &Value, state: &mut H, queue: &mut VecDeque<Value>, room: usize) {
    use crate::value::heap::{deref, HeapObject};

    // An integer and a float of one value are one key, so every number
    // hashes as the bits of its `f64`, under no kind but this marker.
    if let Some(f) = v.as_number() {
        0xFFu8.hash(state);
        f.to_bits().hash(state);
        return;
    }

    let kind = Kind::of(v);
    kind.hash(state);
    let mut push = |child: Value| {
        if queue.len() < room {
            queue.push_back(child);
        }
    };

    match kind {
        Kind::Number => unreachable!("a number returned above"),
        Kind::Nil | Kind::EmptyList => {}
        Kind::Bool => v.tag.hash(state),
        Kind::Symbol | Kind::Keyword | Kind::Pointer | Kind::NativeFn => v.payload.hash(state),
        Kind::String | Kind::Bytes => with_bytes(v, |bytes| bytes.hash(state)),
        Kind::Array => with_elements(v, |elements| {
            elements.len().hash(state);
            elements.iter().for_each(|e| push(*e));
        }),
        Kind::Struct => with_entries(v, |entries| {
            let mut count = 0usize;
            for (k, value) in entries {
                push(k.to_value());
                push(value);
                count += 1;
            }
            count.hash(state);
        }),
        Kind::Set => with_members(v, |members| {
            let mut count = 0usize;
            for member in members {
                push(member);
                count += 1;
            }
            count.hash(state);
        }),

        // SAFETY: every kind below is a heap kind, so the value points at a
        // heap object.
        _ => match unsafe { deref(*v) } {
            HeapObject::Pair(p) => {
                push(p.first);
                push(p.rest);
            }
            HeapObject::LBox { cell, .. } | HeapObject::CaptureCell { cell, .. } => {
                push(cell.get())
            }
            HeapObject::LibHandle(id) => id.hash(state),
            HeapObject::FFISignature(sig, _) => sig.hash(state),
            HeapObject::FFIType(desc) => desc.hash(state),

            // A fiber, a thread and an external hash the handle each wraps,
            // never the slot, because `with-traits` mints a second slot over
            // one handle (repr/eq.rs). Keep these in step with the
            // `PartialEq` arms there.
            HeapObject::Fiber { handle, .. } => handle.id().hash(state),
            HeapObject::ThreadHandle { handle, .. } => {
                (std::sync::Arc::as_ptr(&handle.result) as usize).hash(state)
            }
            HeapObject::External { obj, .. } => {
                (std::rc::Rc::as_ptr(&obj.data) as *const () as usize).hash(state)
            }

            // A closure, a managed pointer, a parameter and a syntax object
            // wrap no shared handle, so the slot is the identity.
            _ => v.payload.hash(state),
        },
    }
}
