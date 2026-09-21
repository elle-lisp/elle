// audited: 2026-09-20
//! Trait table primitives: attach a table, read it back, and resolve the
//! method a collection operator dispatches through.
//! docs/traits.md
//!
//! `with-traits` attaches an immutable struct as a trait table to a value,
//! returning a new heap object with the same data and the given table.
//!
//! `traits` returns the trait table attached to a value, or `nil` if none.
//!
//! `trait/method`, `trait/op` and `trait/iterable?` answer what the Elle-level
//! collection operators ask before they run: which method this value's own
//! table names for this operator, and whether its elements come from `:iter`.
//! The access primitives (`first`, `length`, …) dispatch through
//! `traitregistry::dispatch_trait_method` instead, which also falls back to the
//! registry default.

use crate::primitives::def::{RegionEffect, RetType};
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
        HeapObject::CaptureCell { cell, origin, .. } => Ok(ctx.alloc(HeapObject::CaptureCell {
            cell: std::rc::Rc::new(std::cell::RefCell::new(*cell.borrow())),
            origin: *origin,
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

/// True for the container families that carry their own traversal.
///
/// A collection operator answers for these itself and never reads their trait
/// table, which is what keeps `(with-traits [1 2 3] …)` mapping as an array
/// and keeps a plain array's cost where it was. Every other value — a struct,
/// a set, a closure, a box — reaches the trait layer.
fn is_builtin_sequence(val: &Value) -> bool {
    if val.is_empty_list() || val.as_syntax().is_some() {
        return true;
    }
    if !val.is_heap() {
        return false;
    }
    use crate::value::heap::HeapTag;
    matches!(
        unsafe { deref(*val) }.tag(),
        HeapTag::Pair
            | HeapTag::LArray
            | HeapTag::LArrayMut
            | HeapTag::LString
            | HeapTag::LStringMut
            | HeapTag::LBytes
            | HeapTag::LBytesMut
            | HeapTag::Syntax
    )
}

/// Read the method whose keyword hashes to `name` from a trait table's
/// `:Sequence` protocol, then from its `:Collection` protocol. `Value::NIL`
/// when neither carries it.
///
/// No fall back to the registry default: what an operator asks is what THIS
/// value's own table says, and the access primitives answer from the default
/// traitset already.
fn table_method(table: Value, name: u64) -> Value {
    use crate::primitives::traitregistry::lookup_keyword_hash;
    if table.is_nil() {
        return Value::NIL;
    }
    const SEQUENCE: u64 = crate::value::keyword::keyword_hash("Sequence");
    const COLLECTION: u64 = crate::value::keyword::keyword_hash("Collection");
    for protocol in [SEQUENCE, COLLECTION] {
        let methods = lookup_keyword_hash(&table, protocol);
        if methods.is_nil() {
            continue;
        }
        let m = lookup_keyword_hash(&methods, name);
        if !m.is_nil() {
            return m;
        }
    }
    Value::NIL
}

/// (trait/method value name) → method or nil
pub(crate) fn prim_trait_method(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let Some(name) = args[1].keyword_hash() else {
        return (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!(
                    "trait/method: name must be a keyword, got {}",
                    args[1].type_name()
                ),
            ),
        );
    };
    let table = crate::primitives::traitregistry::get_traitset(&args[0]);
    (SIG_OK, table_method(table, name))
}

/// (trait/op value name) → method or nil
pub(crate) fn prim_trait_op(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if is_builtin_sequence(&args[0]) {
        return (SIG_OK, Value::NIL);
    }
    prim_trait_method(ctx, args)
}

/// (trait/iterable? value) → bool
pub(crate) fn prim_trait_iterable(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if is_builtin_sequence(&args[0]) {
        return (SIG_OK, Value::FALSE);
    }
    const ITER: u64 = crate::value::keyword::keyword_hash("iter");
    let table = crate::primitives::traitregistry::get_traitset(&args[0]);
    let found = !table_method(table, ITER).is_nil();
    (SIG_OK, if found { Value::TRUE } else { Value::FALSE })
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
    "trait/method" => prim_trait_method {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Return a value's trait method by name, read from its :Sequence protocol and then its :Collection protocol. Returns nil when the value's own table carries neither the protocol nor the method.",
        params: &["value", "name"],
        category: "traits",
        example: "(trait/method [1 2 3] :first)",
        // A read that hands back a method the traits table already holds:
        // unbounded result, no argument stored. `Opaque` answers both, as it
        // does for the `first`/`rest` dispatchers next door.
        effect: RegionEffect::Opaque,
    }
    "trait/op" => prim_trait_op {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Return the trait method that overrides a collection operator on a value, or nil. A list, array, string, bytes or syntax object answers nil — those carry their own traversal, so an operator never reads their trait table.",
        params: &["value", "name"],
        category: "traits",
        example: "(trait/op (with-traits {:a 1} @{:Sequence {:map (fn [self f] :mapped)}}) :map)",
        // `trait/method` behind a tag test, and `Opaque` for the same reason.
        effect: RegionEffect::Opaque,
    }
    "trait/iterable?" => prim_trait_iterable {
        ret: RetType::Bool,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "True when a value's elements come from an :iter trait method rather than from a builtin container. A list, array, string, bytes, or a plain set or struct answers false.",
        params: &["value"],
        category: "traits",
        example: "(trait/iterable? [1 2 3])",
        effect: RegionEffect::Immediate,
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
