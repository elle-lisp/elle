// audited: 2026-10-06
//! `CodeUnit` and `CodePin`: the Rust-held references to a code region — a compile's own, and a cache entry's.
//!
//! docs/impl/region/template.md

use std::collections::HashMap;

use crate::hir::region::RuntimeRegion;
use crate::signals::Signal;
use crate::value::fiberheap::FiberHeap;
use crate::value::Value;

use super::{ClosureTemplate, CodeArena, PayloadParts};

/// One compiled unit: the entry function, whose payload's child table holds
/// the lambdas the unit builds, and one counted reference to the code region
/// every payload of the unit lives in.
///
/// RAII: cloning takes a reference and dropping gives one back, both through
/// the heap's code-hold registry. A unit must not outlive its heap.
pub struct CodeUnit {
    arena: CodeArena,
    entry: ClosureTemplate,
    /// Keyword field name → signal of each exported closure, when the unit is
    /// a file whose value is a projectable struct.
    signal_projection: Option<HashMap<String, Signal>>,
}

impl std::fmt::Debug for CodeUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodeUnit")
            .field("region", &self.arena.region())
            .field("entry", &self.entry)
            .finish()
    }
}

impl CodeUnit {
    /// The unit an emission into `arena` produced, entered through
    /// `bytecode`, the entry function's buffer. Takes the reference the code
    /// region was born with.
    pub fn new(arena: CodeArena, mut bytecode: crate::compiler::Bytecode) -> Self {
        let signal_projection = bytecode.signal_projection.take();
        Self::of_parts(arena, PayloadParts::entry(bytecode), signal_projection)
    }

    /// The unit whose entry is written from `entry` into `arena`, whose
    /// children are already there. Takes the reference the code region was
    /// born with.
    pub(crate) fn of_parts(
        arena: CodeArena,
        entry: PayloadParts,
        signal_projection: Option<HashMap<String, Signal>>,
    ) -> Self {
        let entry = ClosureTemplate::new(arena.write(entry));
        unsafe { &mut *arena.heap_ptr() }.hold_code(arena.region(), true);
        CodeUnit {
            arena,
            entry,
            signal_projection,
        }
    }

    /// The entry function's code object: a nullary function with no LIR,
    /// whose child table holds the lambdas the entry builds.
    pub fn entry(&self) -> &ClosureTemplate {
        &self.entry
    }

    /// Keyword field name → signal of each exported closure, when the unit is
    /// a file whose value is a projectable struct.
    pub fn signal_projection(&self) -> Option<&HashMap<String, Signal>> {
        self.signal_projection.as_ref()
    }

    /// The code region every payload of the unit lives in.
    pub fn region(&self) -> RuntimeRegion {
        self.arena.region()
    }

    /// The unit's code region as an arena, for a writer that adds a payload
    /// the unit's region keeps: the WASM host's per-closure code objects.
    #[cfg(feature = "wasm")]
    pub(crate) fn arena(&self) -> CodeArena {
        self.arena
    }

    /// This unit as one on `heap`: itself when its region is `heap`'s, and a
    /// copy written into a fresh code region of `heap` when it is not
    /// (docs/impl/region/template.md § "A unit runs on the heap it was
    /// compiled on").
    pub fn on_heap(&self, heap: &mut FiberHeap) -> CodeUnit {
        if std::ptr::eq(self.arena.heap_ptr(), heap) {
            return self.clone();
        }
        let arena = CodeArena::mint(heap);
        let mut copied: HashMap<usize, Value> = HashMap::new();
        let entry = copy_parts(&self.entry, arena, &mut copied);
        Self::of_parts(arena, entry, self.signal_projection.clone())
    }
}

/// `t`'s fields for a copy into `arena`, its child table rewritten to copies
/// of its children. `copied` maps a source payload to its copy's header, so a
/// payload two parents name copies once.
fn copy_parts(
    t: &ClosureTemplate,
    arena: CodeArena,
    copied: &mut HashMap<usize, Value>,
) -> PayloadParts {
    let mut parts = PayloadParts::of_payload(t);
    parts.children = (0..t.num_children())
        .map(|i| {
            let child = t.child(i);
            let key = child.payload_backing() as usize;
            if let Some(&header) = copied.get(&key) {
                return header;
            }
            let payload = arena.write(copy_parts(&child, arena, copied));
            let header = arena.header(payload);
            copied.insert(key, header);
            header
        })
        .collect();
    parts
}

impl Clone for CodeUnit {
    fn clone(&self) -> Self {
        unsafe { &mut *self.arena.heap_ptr() }.hold_code(self.arena.region(), false);
        CodeUnit {
            arena: self.arena,
            entry: self.entry.clone(),
            signal_projection: self.signal_projection.clone(),
        }
    }
}

impl Drop for CodeUnit {
    fn drop(&mut self) {
        unsafe { &mut *self.arena.heap_ptr() }.release_code(self.arena.region());
    }
}

/// One counted reference to the code region a code object's payload lives
/// in, held by a cache keyed by that payload's bytecode address
/// (docs/impl/jit.md § "Cache identity"). While the pin lives the region
/// cannot free, so the address cannot name another function.
///
/// The pin carries the key it was made for, so a cache derives its key from
/// the pin and never takes one beside it.
///
/// A payload outside the heap's own regions — one a standalone compile left on
/// a heap that lives for the process — needs no pin, and gets an inert one.
pub struct CodePin {
    heap: *mut FiberHeap,
    region: Option<RuntimeRegion>,
    key: *const u8,
}

impl CodePin {
    /// Pin the code region `t`'s payload lives in, on `heap`.
    pub(crate) fn of(heap: &mut FiberHeap, t: &ClosureTemplate) -> CodePin {
        let backing = Value::from_heap_ptr(t.payload_backing(), crate::value::repr::TAG_ARRAY);
        let region = if heap.value_in_region_store(backing) {
            RuntimeRegion::new(heap.region_of_ptr(t.payload_backing()))
        } else {
            None
        };
        if let Some(region) = region {
            heap.hold_code(region, false);
        }
        CodePin {
            heap: heap as *mut FiberHeap,
            region,
            key: t.bytecode().as_ptr(),
        }
    }

    /// The bytecode address of the code object this pin was made for: the key
    /// of the cache entry that holds it.
    pub fn key(&self) -> *const u8 {
        self.key
    }
}

impl Clone for CodePin {
    fn clone(&self) -> Self {
        if let Some(region) = self.region {
            unsafe { &mut *self.heap }.hold_code(region, false);
        }
        CodePin {
            heap: self.heap,
            region: self.region,
            key: self.key,
        }
    }
}

impl Drop for CodePin {
    fn drop(&mut self) {
        if let Some(region) = self.region {
            unsafe { &mut *self.heap }.release_code(region);
        }
    }
}

impl std::fmt::Debug for CodePin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CodePin({:?}, {:p})", self.region, self.key)
    }
}
