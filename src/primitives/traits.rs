//! Trait table primitives: `with-traits` and `traits`.
//!
//! `with-traits` attaches an immutable struct as a trait table to a value,
//! returning a new heap object with the same data and the given table.
//!
//! `traits` returns the trait table attached to a value, or `nil` if none.

use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::heap::{deref, HeapObject};
use crate::value::types::Arity;
use crate::value::Value;

/// (with-traits value table) → new value with trait table attached
///
/// - value must be one of the 19 traitable heap types
/// - table must be an immutable struct (LStruct)
/// - returns a new heap object with the same data and traits = table
/// - for mutable collections the store is COPIED, so the result is
///   independent of the original (see `clone_with_traits`)
pub(crate) fn prim_with_traits(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let value = args[0];
    let table = args[1];

    // Validate: value must be a heap-allocated traitable type
    if !value.is_heap() {
        return (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!(
                    "with-traits: value must be a traitable heap type, got {}",
                    value.type_name()
                ),
            ),
        );
    }

    // Validate: table must be a struct (LStruct or LStructMut)
    if !table.is_heap() || {
        let tag = unsafe { deref(table) }.tag();
        tag != crate::value::heap::HeapTag::LStruct
            && tag != crate::value::heap::HeapTag::LStructMut
    } {
        return (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!(
                    "with-traits: trait table must be a struct, got {}",
                    table.type_name()
                ),
            ),
        );
    }

    // Clone the heap object with new traits
    match unsafe { clone_with_traits(ctx, value, table) } {
        Ok(v) => (SIG_OK, v),
        Err(msg) => (SIG_ERROR, ctx.error("type-error", msg)),
    }
}

/// Clone a heap value, replacing the traits field with `table`.
///
/// The copy is independent for every type that owns its data: mutable
/// collections (LArrayMut, LStructMut, LStringMut, LBytesMut, LSetMut, LBox,
/// CaptureCell) get a fresh `Rc` over a cloned store, and slice-backed
/// immutables get their payload copied into the clone's own region. A write
/// to the original is never visible through the traited copy — the user asked
/// for a value with different traits, not a second name for this one.
///
/// Fiber, ThreadHandle, and External are the exception, and must be: they
/// wrap a shared handle rather than owning data, so the clone names the same
/// fiber, thread, or plugin object. Value identity follows the handle, so the
/// two wrappers stay equal (`repr/eq.rs`, "Wrapper variants take their
/// identity from the handle").
///
/// For infrastructure types (Float, NativeFn, LibHandle, FFISignature,
/// FFIType), returns Err.
///
/// # Safety
/// `value` must be a valid heap pointer.
///
/// The clone (and any slice payload it copies) is born in the native call's own
/// region via `ctx` (RegionEffect::Fresh).
unsafe fn clone_with_traits(
    ctx: &crate::primitives::ctx::NativeCtx<'_>,
    value: Value,
    table: Value,
) -> Result<Value, String> {
    match deref(value) {
        // Slice-backed immutables (LString/LArray/LBytes/LSet): COPY the
        // payload into the clone's own region. RegionSlice is Copy, and
        // copying the (ptr, len) pair instead aliases backing pages in the
        // SOURCE's region with no counted edge — the source's ordinary
        // demise frees the payload under the live clone, and the declared
        // Fresh effect is falsified (docs/impl/region/model.md, "RegionSlice contents
        // share their object's region"; the with-traits UAF,
        // tests/elle/region-withtraits-slice-uaf.lisp).
        HeapObject::LString { s, .. } => Ok(ctx.alloc(HeapObject::LString {
            s: ctx.alloc_slice::<u8>(s.as_slice()),
            traits: table,
        })),
        HeapObject::Pair(pair) => Ok(ctx.alloc(HeapObject::Pair(crate::value::heap::Pair {
            first: pair.first,
            rest: pair.rest,
            traits: table,
        }))),
        // Mutable collections: materialize a fresh Rc over a cloned inner
        // value, so the traited copy is INDEPENDENT — a push to the original
        // is not visible through it. Cloning the Rc instead would share one
        // store between two values the user sees as separate.
        HeapObject::LArrayMut { data, .. } => Ok(ctx.alloc(HeapObject::LArrayMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(data.borrow().clone())),
            traits: table,
        })),
        HeapObject::LStructMut { data, .. } => {
            let entries: std::collections::BTreeMap<_, _> = data
                .borrow()
                .iter()
                .map(|(k, v)| (ctx.intern_key(k), *v))
                .collect();
            Ok(ctx.alloc(HeapObject::LStructMut {
                data: std::rc::Rc::new(std::cell::RefCell::new(entries)),
                traits: table,
            }))
        }
        HeapObject::LStruct { data, .. } => {
            // Keys are interned into the clone's region like the entry slice
            // itself: a traited copy that kept the source's key strings would
            // pin the source's region for its whole life.
            let entries: Vec<(crate::value::heap::TableKey, Value)> =
                data.iter().map(|(k, v)| (ctx.intern_key(k), *v)).collect();
            Ok(ctx.alloc(HeapObject::LStruct {
                data: ctx.alloc_slice::<(crate::value::heap::TableKey, Value)>(&entries),
                traits: table,
            }))
        }
        HeapObject::Closure { closure, .. } => Ok(ctx.alloc(HeapObject::Closure {
            closure: closure.clone(),
            traits: table,
        })),
        HeapObject::LArray { elements, .. } => Ok(ctx.alloc(HeapObject::LArray {
            elements: ctx.alloc_slice::<crate::value::Value>(elements.as_slice()),
            traits: table,
        })),
        HeapObject::LStringMut { data, .. } => Ok(ctx.alloc(HeapObject::LStringMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(data.borrow().clone())),
            traits: table,
        })),
        HeapObject::LBytes { data, .. } => Ok(ctx.alloc(HeapObject::LBytes {
            data: ctx.alloc_slice::<u8>(data.as_slice()),
            traits: table,
        })),
        HeapObject::LBytesMut { data, .. } => Ok(ctx.alloc(HeapObject::LBytesMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(data.borrow().clone())),
            traits: table,
        })),
        HeapObject::LBox { cell, .. } => Ok(ctx.alloc(HeapObject::LBox {
            cell: std::rc::Rc::new(std::cell::RefCell::new(*cell.borrow())),
            traits: table,
        })),
        HeapObject::CaptureCell { cell, .. } => Ok(ctx.alloc(HeapObject::CaptureCell {
            cell: std::rc::Rc::new(std::cell::RefCell::new(*cell.borrow())),
            traits: table,
        })),
        HeapObject::Fiber { handle, .. } => Ok(ctx.alloc(HeapObject::Fiber {
            handle: handle.clone(),
            traits: table,
        })),
        // COPY the tree, do not share it: a `Syntax` node is `Copy`, so
        // assigning one would leave this clone's child slices and string
        // payloads in the SOURCE's region — freed-page reads once the source
        // dies. Every slice-backed arm above copies for the same reason (see
        // `RegionSlice`'s module docs and
        // tests/elle/region-withtraits-slice-uaf.lisp).
        HeapObject::Syntax { syntax, .. } => {
            let owned = syntax.copy_into(&ctx.syntax_arena());
            Ok(ctx.alloc(HeapObject::Syntax {
                syntax: owned,
                traits: table,
            }))
        }
        HeapObject::ManagedPointer { addr, .. } => Ok(ctx.alloc(HeapObject::ManagedPointer {
            addr: std::cell::Cell::new(addr.get()),
            traits: table,
        })),
        HeapObject::External { obj, .. } => Ok(ctx.alloc(HeapObject::External {
            obj: obj.clone(),
            traits: table,
        })),
        HeapObject::Parameter { id, default, .. } => Ok(ctx.alloc(HeapObject::Parameter {
            id: *id,
            default: *default,
            traits: table,
        })),
        HeapObject::ThreadHandle { handle, .. } => Ok(ctx.alloc(HeapObject::ThreadHandle {
            handle: handle.clone(),
            traits: table,
        })),
        HeapObject::LSet { data, .. } => Ok(ctx.alloc(HeapObject::LSet {
            data: ctx.alloc_slice::<crate::value::Value>(data.as_slice()),
            traits: table,
        })),
        HeapObject::LSetMut { data, .. } => Ok(ctx.alloc(HeapObject::LSetMut {
            data: std::rc::Rc::new(std::cell::RefCell::new(data.borrow().clone())),
            traits: table,
        })),
        // Infrastructure types — no trait field; return error. A closure
        // template is never user-visible, so `with-traits` can never reach it.
        HeapObject::Float(_)
        | HeapObject::LibHandle(_)
        | HeapObject::FFISignature(_, _)
        | HeapObject::FFIType(_)
        | HeapObject::ClosureTemplate(_) => Err(format!(
            "with-traits: cannot attach traits to infrastructure type {}",
            deref(value).type_name()
        )),
    }
}

/// (traits value) → trait table or nil
///
/// Returns the trait table attached to value. Since traits are stamped at
/// allocation for collection types, this simply reads the traits field.
/// Returns nil for immediates and infrastructure types.
pub(crate) fn prim_traits(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    (
        SIG_OK,
        crate::primitives::traitregistry::get_traitset(&args[0]),
    )
}

primitive! {
    "with-traits" => prim_with_traits {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Attach a trait table to a value. Returns a new value with the same data and the given trait table. The table must be a struct (immutable or mutable).",
        params: &["value", "table"],
        category: "traits",
        example: "(with-traits [1 2 3] {:Seq {:first (fn (v) (get v 0))}})",
        effect: RegionEffect::Fresh,
        // Among the arguments, only the arg-1 table is a cross-region reference the
        // result's OWN region holds — in its `traits` side-field. Arg 0 is cloned into an
        // independent result whose payload lives in the clone's own region (copied for a
        // slice-backed immutable, deep-cloned for a mutable — see `clone_with_traits`), so
        // the result never references arg 0's region. Declaring `&[1]` makes the region
        // walk record `result ⊇ table`, so the ownership forest sees a captured table flow
        // out through an escaping traited value and keeps it Shared instead of adopting it
        // (region/effects.md § "Native region effects").
        embeds: &[1],
    }
    "traits" => prim_traits {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the trait table attached to a value, or nil if none. Usable as boolean: (if (traits v) ...) checks for presence.",
        params: &["value"],
        category: "traits",
        example: "(traits (with-traits [1 2 3] {:Seq {:first (fn (v) (get v 0))}}))",
        effect: RegionEffect::PassThrough,
    }
}

#[cfg(test)]
mod tests {
    use crate::syntax::SyntaxHeap;
    use crate::value::arena::region_of;

    /// `with-traits` on a syntax object copies the tree into the clone's own
    /// region, like every other slice-backed arm of `clone_with_traits`.
    ///
    /// The counter-factual, and the reason this is a Rust test rather than an
    /// Elle one: a `Syntax` node is `Copy`, so writing `syntax: *syntax`
    /// compiles and leaves the clone's children and name bytes in the SOURCE's
    /// region. Nothing observable happens until that region is freed, which
    /// the region solver defers past every shape an Elle test can write — so
    /// the assertion is on ownership, where the mistake is always visible.
    #[test]
    fn with_traits_copies_a_syntax_tree_into_the_clones_region() {
        let mut vm = crate::vm::VM::new();
        let vm_ptr: *mut crate::vm::VM = &mut vm as *mut _;
        let heap_ptr = vm.heap_ptr;
        let heap = unsafe { &mut *heap_ptr };

        // The source value lives in its own region, read through a scratch
        // heap that is dropped before the clone is even built.
        let source_region = heap.new_runtime_region();
        let source = {
            let mut scratch = SyntaxHeap::new();
            let read = crate::reader::read_syntax(scratch.arena(), "(alpha beta)", "<t>").unwrap();
            crate::value::build::syntax(heap, read, source_region)
        };

        // The clone is built through a ctx over a different region.
        let clone_region = unsafe { (*heap_ptr).new_runtime_region() };
        let clone = {
            let mut ctx = crate::primitives::ctx::NativeCtx::with_region_vm(
                clone_region,
                unsafe { &mut *heap_ptr },
                vm_ptr,
            );
            let table = ctx.struct_from(std::collections::BTreeMap::new());
            let (bits, v) = super::prim_with_traits(&mut ctx, &[source, table]);
            assert_eq!(bits, crate::value::fiber::SIG_OK, "with-traits succeeds");
            v
        };

        let heap = unsafe { &mut *heap_ptr };
        assert_eq!(region_of(heap, clone), Some(clone_region));
        let tree = clone.as_syntax().expect("a syntax value");
        let kids = tree.kind.children();
        assert_eq!(kids.len(), 2);
        assert_eq!(kids[0].as_symbol(), Some("alpha"));
        assert_eq!(
            heap.region_of_ptr(kids.as_ptr() as *const ()),
            clone_region.get(),
            "the clone's children must live in the clone's region, not the source's"
        );

        // The source keeps its own tree, in its own region.
        let src_tree = source.as_syntax().expect("a syntax value");
        assert_eq!(
            heap.region_of_ptr(src_tree.kind.children().as_ptr() as *const ()),
            source_region.get()
        );
    }
}
