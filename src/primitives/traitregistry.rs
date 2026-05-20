//! Thread-local trait registry: default traitsets stamped at allocation.
//!
//! Each collection/sequence HeapTag has a shared @struct traitset.
//! Constructors read the registry entry and stamp it into the new
//! object's `traits` field. All instances of a type share the same
//! @struct pointer by default.

use std::cell::Cell;
use std::collections::BTreeMap;

use crate::value::heap::HeapTag;
use crate::value::types::TableKey;
use crate::value::Value;

/// Number of HeapTag variants (max index is CaptureCell = 28).
const NUM_TAGS: usize = 29;

thread_local! {
    static DEFAULT_TRAITS: Cell<*const [Value; NUM_TAGS]> =
        const { Cell::new(std::ptr::null()) };
}

/// Return the default traitset for a given HeapTag.
/// Returns `Value::NIL` if the registry is not initialized or the tag
/// has no default traitset.
#[inline]
pub fn default_traits_for(tag: HeapTag) -> Value {
    DEFAULT_TRAITS.with(|cell| {
        let ptr = cell.get();
        if ptr.is_null() {
            Value::NIL
        } else {
            unsafe { (*ptr)[tag as usize] }
        }
    })
}

/// Initialize the thread-local default traits registry.
///
/// Leaks a `Box<[Value; NUM_TAGS]>` — same pattern as the root heap.
/// Called from VM initialization before any allocations happen.
pub fn init_default_traits() {
    DEFAULT_TRAITS.with(|cell| {
        if !cell.get().is_null() {
            return; // already initialized
        }
        let mut table = [Value::NIL; NUM_TAGS];

        // Build method structs for :Sequence and :Collection protocols,
        // then assemble @struct traitsets for each collection type.

        // ── :Sequence methods (immutable struct) ────────────────────
        let seq_methods = build_sequence_methods();

        // ── :Collection methods (immutable struct) ──────────────────
        let coll_methods = build_collection_methods();

        // ── Build traitsets ─────────────────────────────────────────

        // Sequence + Collection types: array, @array, list, string,
        // @string, bytes, @bytes
        let seq_coll = make_traitset(Some(seq_methods), Some(coll_methods));

        // Collection-only types: set, @set, struct, @struct
        let coll_only = make_traitset(None, Some(coll_methods));

        // Stamp entries
        table[HeapTag::LArray as usize] = seq_coll;
        table[HeapTag::LArrayMut as usize] = seq_coll;
        table[HeapTag::Pair as usize] = seq_coll;
        table[HeapTag::LString as usize] = seq_coll;
        table[HeapTag::LStringMut as usize] = seq_coll;
        table[HeapTag::LBytes as usize] = seq_coll;
        table[HeapTag::LBytesMut as usize] = seq_coll;
        table[HeapTag::LSet as usize] = coll_only;
        table[HeapTag::LSetMut as usize] = coll_only;
        table[HeapTag::LStruct as usize] = coll_only;
        table[HeapTag::LStructMut as usize] = coll_only;

        let leaked = Box::leak(Box::new(table));
        cell.set(leaked as *const [Value; NUM_TAGS]);
    });
}

/// Build the :Sequence method struct (immutable).
fn build_sequence_methods() -> Value {
    use crate::primitives::def::PrimitiveDef;
    use crate::signals::Signal;
    use crate::value::types::Arity;

    // Native function definitions for sequence methods.
    // These are leaked static refs, same pattern as permanent NativeFns.
    static SEQ_FIRST: PrimitiveDef = PrimitiveDef {
        name: "trait:Sequence:first",
        func: trait_seq_first,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Sequence trait: first element",
        params: &["self"],
        category: "trait",
        example: "",
        aliases: &[],
        ..PrimitiveDef::DEFAULT
    };
    static SEQ_REST: PrimitiveDef = PrimitiveDef {
        name: "trait:Sequence:rest",
        func: trait_seq_rest,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Sequence trait: rest of sequence",
        params: &["self"],
        category: "trait",
        example: "",
        aliases: &[],
        ..PrimitiveDef::DEFAULT
    };
    static SEQ_LAST: PrimitiveDef = PrimitiveDef {
        name: "trait:Sequence:last",
        func: trait_seq_last,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Sequence trait: last element",
        params: &["self"],
        category: "trait",
        example: "",
        aliases: &[],
        ..PrimitiveDef::DEFAULT
    };
    static SEQ_NTH: PrimitiveDef = PrimitiveDef {
        name: "trait:Sequence:nth",
        func: trait_seq_nth,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Sequence trait: nth element",
        params: &["self", "n"],
        category: "trait",
        example: "",
        aliases: &[],
        ..PrimitiveDef::DEFAULT
    };
    static SEQ_ITER: PrimitiveDef = PrimitiveDef {
        name: "trait:Sequence:iter",
        func: trait_seq_iter,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Sequence trait: fiber iterator",
        params: &["self"],
        category: "trait",
        example: "",
        aliases: &[],
        ..PrimitiveDef::DEFAULT
    };

    let mut entries = BTreeMap::new();
    entries.insert(
        TableKey::Keyword("first".into()),
        Value::native_fn(&SEQ_FIRST),
    );
    entries.insert(
        TableKey::Keyword("rest".into()),
        Value::native_fn(&SEQ_REST),
    );
    entries.insert(
        TableKey::Keyword("last".into()),
        Value::native_fn(&SEQ_LAST),
    );
    entries.insert(TableKey::Keyword("nth".into()), Value::native_fn(&SEQ_NTH));
    entries.insert(
        TableKey::Keyword("iter".into()),
        Value::native_fn(&SEQ_ITER),
    );
    alloc_permanent_struct(entries)
}

/// Build the :Collection method struct (immutable).
///
/// All collection types currently share the same native implementations
/// (coll_len, coll_empty, coll_has dispatch internally on type). If
/// collection types ever need divergent method sets, split this back
/// into per-category builders.
fn build_collection_methods() -> Value {
    use crate::primitives::def::PrimitiveDef;
    use crate::signals::Signal;
    use crate::value::types::Arity;

    static COLL_LENGTH: PrimitiveDef = PrimitiveDef {
        name: "trait:Collection:length",
        func: trait_coll_length,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Collection trait: element count",
        params: &["self"],
        category: "trait",
        example: "",
        aliases: &[],
        ..PrimitiveDef::DEFAULT
    };
    static COLL_EMPTY: PrimitiveDef = PrimitiveDef {
        name: "trait:Collection:empty?",
        func: trait_coll_empty,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Collection trait: is empty?",
        params: &["self"],
        category: "trait",
        example: "",
        aliases: &[],
        ..PrimitiveDef::DEFAULT
    };
    static COLL_HAS: PrimitiveDef = PrimitiveDef {
        name: "trait:Collection:has?",
        func: trait_coll_has,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Collection trait: membership test",
        params: &["self", "needle"],
        category: "trait",
        example: "",
        aliases: &[],
        ..PrimitiveDef::DEFAULT
    };
    static COLL_CONJ: PrimitiveDef = PrimitiveDef {
        name: "trait:Collection:conj",
        func: trait_coll_conj,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Collection trait: add element",
        params: &["self", "item"],
        category: "trait",
        example: "",
        aliases: &[],
        ..PrimitiveDef::DEFAULT
    };
    static COLL_EMPTY_NEW: PrimitiveDef = PrimitiveDef {
        name: "trait:Collection:empty",
        func: trait_coll_empty_new,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Collection trait: empty container of same type",
        params: &["self"],
        category: "trait",
        example: "",
        aliases: &[],
        ..PrimitiveDef::DEFAULT
    };

    let mut entries = BTreeMap::new();
    entries.insert(
        TableKey::Keyword("length".into()),
        Value::native_fn(&COLL_LENGTH),
    );
    entries.insert(
        TableKey::Keyword("empty?".into()),
        Value::native_fn(&COLL_EMPTY),
    );
    entries.insert(
        TableKey::Keyword("has?".into()),
        Value::native_fn(&COLL_HAS),
    );
    entries.insert(
        TableKey::Keyword("conj".into()),
        Value::native_fn(&COLL_CONJ),
    );
    entries.insert(
        TableKey::Keyword("empty".into()),
        Value::native_fn(&COLL_EMPTY_NEW),
    );
    alloc_permanent_struct(entries)
}

/// Allocate an immutable struct as a permanent (Rc-backed) allocation.
/// Permanent allocations are never reclaimed by arena scope operations,
/// so they are safe to reference from any scope.
fn alloc_permanent_struct(entries: BTreeMap<TableKey, Value>) -> Value {
    use crate::value::heap::{alloc_permanent, HeapObject};
    let sorted: Vec<(TableKey, Value)> = entries.into_iter().collect();
    alloc_permanent(HeapObject::LStruct {
        data: sorted,
        traits: Value::NIL,
    })
}

/// Allocate a mutable struct as a permanent (Rc-backed) allocation.
fn alloc_permanent_struct_mut(entries: BTreeMap<TableKey, Value>) -> Value {
    use crate::value::heap::{alloc_permanent, HeapObject};
    use std::cell::RefCell;
    use std::rc::Rc;
    alloc_permanent(HeapObject::LStructMut {
        data: Rc::new(RefCell::new(entries)),
        traits: Value::NIL,
    })
}

/// Build a traitset @struct from optional protocol method structs.
fn make_traitset(sequence: Option<Value>, collection: Option<Value>) -> Value {
    let mut entries = BTreeMap::new();
    if let Some(seq) = sequence {
        entries.insert(TableKey::Keyword("Sequence".into()), seq);
    }
    if let Some(coll) = collection {
        entries.insert(TableKey::Keyword("Collection".into()), coll);
    }
    alloc_permanent_struct_mut(entries)
}

// ── Trait method implementations ────────────────────────────────────

use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};

fn trait_seq_first(args: &[Value]) -> (SignalBits, Value) {
    match super::seq::seq_first(&args[0]) {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, e),
    }
}

fn trait_seq_rest(args: &[Value]) -> (SignalBits, Value) {
    match super::seq::seq_rest(&args[0]) {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, e),
    }
}

fn trait_seq_last(args: &[Value]) -> (SignalBits, Value) {
    match super::seq::seq_last(&args[0]) {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, e),
    }
}

fn trait_seq_nth(args: &[Value]) -> (SignalBits, Value) {
    let n = match args[1].as_int() {
        Some(n) => n,
        None => {
            return (
                SIG_ERROR,
                crate::value::error_val(
                    "type-error",
                    format!("nth: expected integer index, got {}", args[1].type_name()),
                ),
            )
        }
    };
    match super::seq::seq_nth(&args[0], n) {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, e),
    }
}

fn trait_seq_iter(args: &[Value]) -> (SignalBits, Value) {
    use crate::value::fiber::Fiber;

    let val = args[0];

    // Collect all elements, then create a native iterator fiber
    // that yields them one by one on each resume.
    let elements = match super::collection::coll_to_vec(&val) {
        Ok(v) => v,
        Err(e) => return (SIG_ERROR, e),
    };

    let mask = crate::signals::SIG_YIELD;
    let fiber = Fiber::native_iter(elements, mask);
    (SIG_OK, Value::fiber(fiber))
}

fn trait_coll_length(args: &[Value]) -> (SignalBits, Value) {
    match super::collection::coll_len(&args[0]) {
        Ok(n) => (SIG_OK, Value::int(n as i64)),
        Err(e) => (SIG_ERROR, e),
    }
}

fn trait_coll_empty(args: &[Value]) -> (SignalBits, Value) {
    match super::collection::coll_empty(&args[0]) {
        Ok(empty) => (SIG_OK, Value::bool(empty)),
        Err(e) => (SIG_ERROR, e),
    }
}

fn trait_coll_has(args: &[Value]) -> (SignalBits, Value) {
    match super::collection::coll_has(&args[0], &args[1]) {
        Ok(found) => (SIG_OK, Value::bool(found)),
        Err(e) => (SIG_ERROR, e),
    }
}

fn trait_coll_conj(args: &[Value]) -> (SignalBits, Value) {
    let coll = &args[0];
    let item = args[1];

    // Array: append
    if let Some(elems) = coll.as_array() {
        let mut new = elems.to_vec();
        new.push(item);
        return (SIG_OK, Value::array(new));
    }
    if let Some(arr) = coll.as_array_mut() {
        arr.borrow_mut().push(item);
        return (SIG_OK, *coll);
    }

    // List: prepend (cons)
    if coll.is_pair() || coll.is_empty_list() {
        return (SIG_OK, Value::pair(item, *coll));
    }

    // Set: add
    if let Some(s) = coll.as_set() {
        let frozen = super::sets::freeze_value(item);
        let mut new: std::collections::BTreeSet<Value> = s.iter().copied().collect();
        new.insert(frozen);
        return (SIG_OK, Value::set(new));
    }
    if let Some(s) = coll.as_set_mut() {
        let frozen = super::sets::freeze_value(item);
        s.borrow_mut().insert(frozen);
        return (SIG_OK, *coll);
    }

    // String: append string
    if coll.is_string() {
        let s = item.with_string(|s| s.to_string()).unwrap_or_default();
        return coll
            .with_string(|base| {
                let mut new = base.to_string();
                new.push_str(&s);
                (SIG_OK, Value::string(new))
            })
            .unwrap_or_else(|| {
                (
                    SIG_ERROR,
                    crate::value::error_val("type-error", "conj: unreachable string case"),
                )
            });
    }

    // Bytes: append byte
    if let Some(b) = coll.as_bytes() {
        if let Some(n) = item.as_int() {
            if (0..=255).contains(&n) {
                let mut new = b.to_vec();
                new.push(n as u8);
                return (SIG_OK, Value::bytes(new));
            }
        }
    }

    (
        SIG_ERROR,
        crate::value::error_val(
            "type-error",
            format!("conj: unsupported collection type {}", coll.type_name()),
        ),
    )
}

fn trait_coll_empty_new(args: &[Value]) -> (SignalBits, Value) {
    let coll = &args[0];

    if coll.as_array().is_some() {
        return (SIG_OK, Value::array(vec![]));
    }
    if coll.as_array_mut().is_some() {
        return (SIG_OK, Value::array_mut(vec![]));
    }
    if coll.is_pair() || coll.is_empty_list() {
        return (SIG_OK, Value::EMPTY_LIST);
    }
    if coll.as_set().is_some() {
        return (SIG_OK, Value::set(std::collections::BTreeSet::new()));
    }
    if coll.as_set_mut().is_some() {
        return (SIG_OK, Value::set_mut(std::collections::BTreeSet::new()));
    }
    if coll.as_struct().is_some() {
        return (
            SIG_OK,
            Value::struct_from(std::collections::BTreeMap::new()),
        );
    }
    if coll.as_struct_mut().is_some() {
        return (SIG_OK, Value::struct_mut());
    }
    if coll.is_string() {
        return (SIG_OK, Value::string(""));
    }
    if coll.as_string_mut().is_some() {
        return (SIG_OK, Value::string_mut(vec![]));
    }
    if coll.as_bytes().is_some() {
        return (SIG_OK, Value::bytes(vec![]));
    }
    if coll.as_bytes_mut().is_some() {
        return (SIG_OK, Value::bytes_mut(vec![]));
    }

    (
        SIG_ERROR,
        crate::value::error_val(
            "type-error",
            format!("empty: unsupported collection type {}", coll.type_name()),
        ),
    )
}

// ── Dispatch helper ─────────────────────────────────────────────────

/// Read the traits field from a value.
///
/// Returns the traits @struct for heap objects, or `Value::NIL` for
/// immediates and infrastructure types. No fallback, no registry lookup.
pub fn get_traitset(val: &Value) -> Value {
    if !val.is_heap() {
        return Value::NIL;
    }
    unsafe {
        match crate::value::heap::deref(*val) {
            crate::value::heap::HeapObject::LString { traits, .. }
            | crate::value::heap::HeapObject::LArray { traits, .. }
            | crate::value::heap::HeapObject::LArrayMut { traits, .. }
            | crate::value::heap::HeapObject::LStruct { traits, .. }
            | crate::value::heap::HeapObject::LStructMut { traits, .. }
            | crate::value::heap::HeapObject::LStringMut { traits, .. }
            | crate::value::heap::HeapObject::LBytes { traits, .. }
            | crate::value::heap::HeapObject::LBytesMut { traits, .. }
            | crate::value::heap::HeapObject::LSet { traits, .. }
            | crate::value::heap::HeapObject::LSetMut { traits, .. }
            | crate::value::heap::HeapObject::Closure { traits, .. }
            | crate::value::heap::HeapObject::LBox { traits, .. }
            | crate::value::heap::HeapObject::CaptureCell { traits, .. }
            | crate::value::heap::HeapObject::Fiber { traits, .. }
            | crate::value::heap::HeapObject::Syntax { traits, .. }
            | crate::value::heap::HeapObject::ManagedPointer { traits, .. }
            | crate::value::heap::HeapObject::External { traits, .. }
            | crate::value::heap::HeapObject::Parameter { traits, .. }
            | crate::value::heap::HeapObject::ThreadHandle { traits, .. } => *traits,
            crate::value::heap::HeapObject::Pair(pair) => pair.traits,
            _ => Value::NIL,
        }
    }
}

/// Look up a trait method on a value and call it.
///
/// Reads the traits field (always populated for collection types),
/// looks up the protocol and method, and calls the method.
/// If the value's trait table doesn't contain the requested protocol,
/// falls back to the default traitset from the registry.
/// Returns `(SignalBits, Value)` directly.
pub fn dispatch_trait_method(
    val: &Value,
    protocol: &str,
    method: &str,
    args: &[Value],
) -> (SignalBits, Value) {
    use crate::value::error_val;

    let traits_val = get_traitset(val);
    if traits_val.is_nil() {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: no trait table on {} value", method, val.type_name()),
            ),
        );
    }

    // Look up protocol in the value's trait table
    let mut protocol_val = lookup_keyword(&traits_val, protocol);

    // If protocol not found in the value's traits, try the default
    // traitset from the registry (user traits may override only some protocols)
    if protocol_val.is_nil() && val.is_heap() {
        let tag = unsafe { crate::value::heap::deref(*val) }.tag();
        let default = default_traits_for(tag);
        if !default.is_nil() {
            protocol_val = lookup_keyword(&default, protocol);
        }
    }

    if protocol_val.is_nil() {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: no :{} protocol on {} value",
                    method,
                    protocol,
                    val.type_name()
                ),
            ),
        );
    }

    let method_fn = lookup_keyword(&protocol_val, method);
    if method_fn.is_nil() {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: no :{} method in :{} protocol",
                    method, method, protocol
                ),
            ),
        );
    }

    call_method_fn(&method_fn, protocol, method, args)
}

/// Call a resolved trait method (NativeFn or Closure).
fn call_method_fn(
    method_fn: &Value,
    protocol: &str,
    method: &str,
    args: &[Value],
) -> (SignalBits, Value) {
    // NativeFn — call directly
    if let Some(prim_fn) = method_fn.as_native_fn() {
        return prim_fn(args);
    }

    // Closure — call via VM context
    if let Some(closure) = method_fn.as_closure() {
        let vm_ptr = match crate::context::get_vm_context() {
            Some(ptr) => ptr,
            None => {
                return (
                    SIG_ERROR,
                    crate::value::error_val(
                        "internal-error",
                        "trait dispatch: no VM context for closure call",
                    ),
                );
            }
        };
        let vm = unsafe { &mut *vm_ptr };
        match vm.call_closure(closure, args) {
            Ok(v) => return (SIG_OK, v),
            Err(msg) => {
                return (SIG_ERROR, crate::value::error_val("trait-error", msg));
            }
        }
    }

    // Not callable
    (
        SIG_ERROR,
        crate::value::error_val(
            "type-error",
            format!(
                "{}:{}: trait method is not callable ({})",
                protocol,
                method,
                method_fn.type_name()
            ),
        ),
    )
}

/// Look up a keyword key in a struct without allocating a TableKey.
///
/// Trait tables are small (2–5 entries), so linear scan on the keyword
/// discriminant + string comparison avoids the String allocation that
/// `TableKey::Keyword(key.into())` would require on every dispatch.
fn lookup_keyword(val: &Value, key: &str) -> Value {
    // Immutable struct — linear scan (small tables)
    if let Some(entries) = val.as_struct() {
        for (k, v) in entries.iter() {
            if let TableKey::Keyword(ref s) = k {
                if s == key {
                    return *v;
                }
            }
        }
        return Value::NIL;
    }

    // Mutable struct — linear scan on values
    if let Some(map_ref) = val.as_struct_mut() {
        let borrowed = map_ref.borrow();
        for (k, v) in borrowed.iter() {
            if let TableKey::Keyword(ref s) = k {
                if s == key {
                    return *v;
                }
            }
        }
        return Value::NIL;
    }

    Value::NIL
}
