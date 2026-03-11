//! SendValue wrapper for thread-safe value transmission
//!
//! This module provides SendValue, a wrapper around Value that implements Send
//! by deep-copying heap values instead of sharing raw pointers.
//!
//! The problem with raw Value copies: NaN-boxed Value contains raw pointers to Rc
//! heap objects. When sent to another thread, the original Rc may drop and free the
//! heap object while the thread still holds a raw pointer to it.
//!
//! The solution: SendValue stores owned copies of heap data, not raw pointers.

use super::heap::{alloc, deref, Cons, HeapObject};
use super::repr::Value;
use crate::effects::Effect;
use crate::error::LocationMap;
use crate::hir::VarargKind;
use crate::value::types::Arity;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Sendable snapshot of a closure.
///
/// All `Rc`-wrapped fields from `ClosureTemplate` are owned here.
/// Fields that are not portable across thread boundaries (`jit_code`,
/// `lir_function`, `syntax`) are absent — they are set to `None` on
/// reconstruction.
///
/// `env` holds the captured environment (upvalues), converted recursively
/// to `SendValue`. Constants are stored separately in `constants`.
///
/// This struct is `pub(crate)` — it is part of the public interface of
/// `SendBundle` but not independently useful outside `send.rs`.
#[derive(Clone)]
pub struct SendableClosure {
    pub bytecode: Vec<u8>,
    pub arity: Arity,
    pub num_locals: usize,
    pub num_captures: usize,
    pub num_params: usize,
    pub constants: Vec<SendValue>,
    pub effect: Effect,
    pub lbox_params_mask: u64,
    pub lbox_locals_mask: u64,
    pub symbol_names: HashMap<u32, String>,
    pub location_map: LocationMap,
    pub doc: Option<Box<SendValue>>,
    pub vararg_kind: VarargKind,
    pub name: Option<String>,
    pub env: Vec<SendValue>,
}

/// A thread-safe wrapper around Value that deep-copies heap data.
///
/// For immediate values (nil, bool, int, float, symbol), SendValue stores
/// them directly. Keywords carry their name for cross-thread re-interning.
/// For heap values, SendValue stores owned copies of the heap data, ensuring
/// the data remains valid even if the original Rc is dropped.
#[derive(Clone)]
pub enum SendValue {
    /// Immediate values that don't need copying
    Immediate(Value),

    /// Keyword with name for cross-thread re-interning
    Keyword(String),

    /// Owned string copy
    String(String),

    /// Deep copy of cons cells
    Cons(Box<SendValue>, Box<SendValue>),

    /// Deep copy of arrays
    Array(Vec<SendValue>),

    /// Deep copy of structs (immutable maps)
    Struct(BTreeMap<crate::value::heap::TableKey, SendValue>),

    /// Deep copy of arrays (immutable fixed-length sequences)
    Tuple(Vec<SendValue>),

    /// Deep copy of @strings (mutable byte sequences)
    Buffer(Vec<u8>),

    /// Deep copy of @bytes (immutable binary data)
    Bytes(Vec<u8>),

    /// Deep copy of @bytes (mutable binary data)
    Blob(Vec<u8>),

    /// Deep copy of mutable boxes (if contents are sendable)
    /// The bool indicates if it's a local lbox (auto-unwrapped) or user box
    LBox(Box<SendValue>, bool),

    /// Float values that couldn't be stored inline
    Float(f64),

    /// Deep copy of FFI type descriptor (pure data, no Rc)
    FFIType(crate::ffi::types::TypeDesc),

    /// Deep copy of immutable sets
    LSet(Vec<SendValue>),

    /// Deep copy of mutable sets
    LSetMut(Vec<SendValue>),

    /// Native function pointer (inherently Send + Sync)
    NativeFn(crate::value::types::NativeFn),

    /// Deep copy of a closure (template + captured environment).
    /// Only appears as an entry in `SendBundle::closures`.
    /// The root `SendValue` tree and closure envs reference closures via `Ref(idx)`.
    Closure(SendableClosure),

    /// Back-reference into `SendBundle::closures` by index.
    /// Meaningful only within a `SendBundle`; a bare `Ref` without a bundle is invalid.
    Ref(usize),
}

/// Unit of cross-thread value transfer.
///
/// All closures reachable from `root` — including nested and mutually recursive
/// ones — are stored flat in `closures`. The root value tree and all closure
/// envs reference closures by index via `SendValue::Ref(idx)`.
///
/// This replaces bare `SendValue` as the type carried by `ThreadHandle::result`.
#[derive(Clone)]
pub struct SendBundle {
    /// Root value. May contain `Ref(idx)` nodes pointing into `closures`.
    pub root: SendValue,
    /// Intern table of all closures reachable from `root`.
    pub closures: Vec<SendableClosure>,
}

// SAFETY: SendBundle owns all its data — no Rc, no RefCell.
unsafe impl Send for SendBundle {}
unsafe impl Sync for SendBundle {}

/// Per-call serialization context for `SendBundle::from_value`.
struct SerContext {
    /// Maps `value.to_bits()` (NaN-boxed heap pointer identity) → intern table index.
    /// Inserted BEFORE recursing into a closure's fields, so back-references find it.
    visited: HashMap<u64, usize>,
    /// Intern table being built.
    closures: Vec<SendableClosure>,
}

/// Recursive worker for serialization. Threads SerContext through all recursive calls.
fn from_value_inner(value: Value, ctx: &mut SerContext) -> Result<SendValue, String> {
    // Keywords carry their name for cross-thread re-interning
    if let Some(name) = value.as_keyword_name() {
        return Ok(SendValue::Keyword(name.to_string()));
    }

    // Immediate values are always safe
    if value.is_nil() || value.is_bool() || value.is_int() || value.is_float() || value.is_symbol()
    {
        return Ok(SendValue::Immediate(value));
    }

    // String values (SSO or heap)
    if let Some(s) = value.with_string(|s| s.to_string()) {
        return Ok(SendValue::String(s));
    }

    // Heap values need deep copying
    if !value.is_heap() {
        return Ok(SendValue::Immediate(value));
    }

    match unsafe { deref(value) } {
        // Strings are immutable and safe
        HeapObject::LString(s) => Ok(SendValue::String(s.to_string())),

        // Cons cells - deep copy both first and rest
        HeapObject::Cons(cons) => {
            let first = from_value_inner(cons.first, ctx)?;
            let rest = from_value_inner(cons.rest, ctx)?;
            Ok(SendValue::Cons(Box::new(first), Box::new(rest)))
        }

        // Arrays - deep copy all elements
        HeapObject::LArrayMut(vec_ref) => {
            let borrowed = vec_ref
                .try_borrow()
                .map_err(|_| "Cannot borrow array for sending".to_string())?;
            let copied: Result<Vec<SendValue>, String> =
                borrowed.iter().map(|v| from_value_inner(*v, ctx)).collect();
            Ok(SendValue::Array(copied?))
        }

        // Structs - deep copy all values
        HeapObject::LStruct(s) => {
            let mut copied = BTreeMap::new();
            for (k, v) in s.iter() {
                if !k.is_sendable() {
                    return Err("Cannot send struct with identity keys".to_string());
                }
                copied.insert(k.clone(), from_value_inner(*v, ctx)?);
            }
            Ok(SendValue::Struct(copied))
        }

        // Arrays (immutable) - deep copy all elements
        HeapObject::LArray(elems) => {
            let copied: Result<Vec<SendValue>, String> =
                elems.iter().map(|v| from_value_inner(*v, ctx)).collect();
            Ok(SendValue::Tuple(copied?))
        }

        // @string - deep copy the bytes
        HeapObject::LStringMut(buf_ref) => {
            let borrowed = buf_ref
                .try_borrow()
                .map_err(|_| "Cannot borrow @string for sending".to_string())?;
            Ok(SendValue::Buffer(borrowed.clone()))
        }

        // Boxes - deep copy the contents if sendable
        HeapObject::LBox(cell_ref, is_local) => {
            let borrowed = cell_ref
                .try_borrow()
                .map_err(|_| "Cannot borrow box for sending".to_string())?;
            let contents = from_value_inner(*borrowed, ctx)?;
            Ok(SendValue::LBox(Box::new(contents), *is_local))
        }

        // Float values that couldn't be stored inline
        HeapObject::Float(f) => Ok(SendValue::Float(*f)),

        // Unsafe: mutable @structs
        HeapObject::LStructMut(_) => Err("Cannot send mutable @struct".to_string()),

        // Closures: intern into the table, with cycle detection via pre-insertion
        HeapObject::Closure(closure_rc) => {
            // Use value.to_bits() as identity key — same heap pointer = same bits.
            let key = value.to_bits();

            // Already visited → return Ref to existing intern entry.
            if let Some(&idx) = ctx.visited.get(&key) {
                return Ok(SendValue::Ref(idx));
            }

            // Reserve an index BEFORE recursing so back-references resolve to this entry.
            let idx = ctx.closures.len();
            // Push a placeholder (will be overwritten below).
            ctx.closures.push(SendableClosure {
                bytecode: Vec::new(),
                arity: closure_rc.template.arity,
                num_locals: 0,
                num_captures: 0,
                num_params: 0,
                constants: Vec::new(),
                effect: closure_rc.template.effect,
                lbox_params_mask: 0,
                lbox_locals_mask: 0,
                symbol_names: HashMap::new(),
                location_map: LocationMap::new(),
                doc: None,
                vararg_kind: closure_rc.template.vararg_kind.clone(),
                name: None,
                env: Vec::new(),
            });
            ctx.visited.insert(key, idx);

            // Serialize environment (may contain back-references to this closure via LBox).
            let env: Result<Vec<SendValue>, String> = closure_rc
                .env
                .iter()
                .map(|v| from_value_inner(*v, ctx))
                .collect();
            let env = env?;

            // Serialize constants.
            let constants: Result<Vec<SendValue>, String> = closure_rc
                .template
                .constants
                .iter()
                .map(|v| from_value_inner(*v, ctx))
                .collect();
            let constants = constants?;

            // Serialize doc (optional).
            let doc = match closure_rc.template.doc {
                Some(d) => Some(Box::new(from_value_inner(d, ctx)?)),
                None => None,
            };

            // Replace placeholder with complete entry.
            ctx.closures[idx] = SendableClosure {
                bytecode: (*closure_rc.template.bytecode).clone(),
                arity: closure_rc.template.arity,
                num_locals: closure_rc.template.num_locals,
                num_captures: closure_rc.template.num_captures,
                num_params: closure_rc.template.num_params,
                constants,
                effect: closure_rc.template.effect,
                lbox_params_mask: closure_rc.template.lbox_params_mask,
                lbox_locals_mask: closure_rc.template.lbox_locals_mask,
                symbol_names: (*closure_rc.template.symbol_names).clone(),
                location_map: (*closure_rc.template.location_map).clone(),
                doc,
                vararg_kind: closure_rc.template.vararg_kind.clone(),
                name: closure_rc.template.name.as_ref().map(|s| s.to_string()),
                env,
            };

            Ok(SendValue::Ref(idx))
        }

        // Native function pointers are inherently Send + Sync
        HeapObject::NativeFn(f) => Ok(SendValue::NativeFn(*f)),

        // Unsafe: FFI handles
        HeapObject::LibHandle(_) => Err("Cannot send library handle".to_string()),

        // Unsafe: thread handles
        HeapObject::ThreadHandle(_) => Err("Cannot send thread handle".to_string()),

        // Unsafe: fibers (contain execution state with closures)
        HeapObject::Fiber(_) => Err("Cannot send fiber".to_string()),

        // Unsafe: syntax objects (contain Rc)
        HeapObject::Syntax(_) => Err("Cannot send syntax object".to_string()),

        // Unsafe: bindings (compile-time only)
        HeapObject::Binding(_) => Err("Cannot send binding".to_string()),

        // Unsafe: FFI signatures (contain non-Send types like Cif)
        HeapObject::FFISignature(_, _) => Err("Cannot send FFI signature".to_string()),

        // Unsafe: managed pointers (lifecycle state is not thread-safe with Cell)
        HeapObject::ManagedPointer(_) => Err("Cannot send managed pointer".to_string()),

        // Unsafe: external objects (contain Rc<dyn Any>, not thread-safe)
        HeapObject::External(_) => Err("Cannot send external object".to_string()),

        // Unsafe: parameters (fiber-local state)
        HeapObject::Parameter { .. } => Err("Cannot send parameter".to_string()),

        // FFI type descriptors are pure data — safe to send
        HeapObject::FFIType(desc) => Ok(SendValue::FFIType(desc.clone())),

        // Bytes - immutable and safe to send
        HeapObject::LBytes(b) => Ok(SendValue::Bytes(b.clone())),

        // @bytes - deep copy the bytes
        HeapObject::LBytesMut(blob_ref) => {
            let borrowed = blob_ref
                .try_borrow()
                .map_err(|_| "Cannot borrow @bytes for sending".to_string())?;
            Ok(SendValue::Blob(borrowed.clone()))
        }

        // Sets (immutable) - deep copy all elements
        HeapObject::LSet(s) => {
            let copied: Result<Vec<SendValue>, String> =
                s.iter().map(|v| from_value_inner(*v, ctx)).collect();
            Ok(SendValue::LSet(copied?))
        }

        // Sets (mutable) - deep copy all elements
        HeapObject::LSetMut(s_ref) => {
            let borrowed = s_ref
                .try_borrow()
                .map_err(|_| "Cannot borrow mutable set for sending".to_string())?;
            let copied: Result<Vec<SendValue>, String> =
                borrowed.iter().map(|v| from_value_inner(*v, ctx)).collect();
            Ok(SendValue::LSetMut(copied?))
        }
    }
}

impl SendValue {
    /// Convert a Value to SendValue by deep-copying heap data.
    ///
    /// Returns Err if the value contains non-sendable data (mutable @structs,
    /// native functions, FFI handles, etc.).
    ///
    /// Note: this wrapper asserts that no closures are encountered. For values
    /// that may contain closures, use `SendBundle::from_value` instead.
    pub fn from_value(value: Value) -> Result<Self, String> {
        let mut ctx = SerContext {
            visited: HashMap::new(),
            closures: Vec::new(),
        };
        let sv = from_value_inner(value, &mut ctx)?;
        debug_assert!(
            ctx.closures.is_empty(),
            "from_value produced closures but caller expects a bare SendValue"
        );
        Ok(sv)
    }

    /// Convert SendValue back into a Value by reconstructing heap objects.
    pub fn into_value(self) -> Value {
        match self {
            SendValue::Immediate(v) => v,
            SendValue::Keyword(name) => Value::keyword(&name),
            SendValue::String(s) => Value::string_no_intern(s),
            SendValue::Cons(first, rest) => {
                let first_val = first.into_value();
                let rest_val = rest.into_value();
                let cons = Cons::new(first_val, rest_val);
                alloc(HeapObject::Cons(cons))
            }
            SendValue::Array(items) => {
                let values: Vec<Value> = items.into_iter().map(|sv| sv.into_value()).collect();
                alloc(HeapObject::LArrayMut(std::cell::RefCell::new(values)))
            }
            SendValue::Struct(map) => {
                let values: BTreeMap<_, _> = map
                    .into_iter()
                    .map(|(k, sv)| (k, sv.into_value()))
                    .collect();
                alloc(HeapObject::LStruct(values))
            }
            SendValue::Tuple(items) => {
                let values: Vec<Value> = items.into_iter().map(|sv| sv.into_value()).collect();
                alloc(HeapObject::LArray(values))
            }
            SendValue::Buffer(bytes) => {
                alloc(HeapObject::LStringMut(std::cell::RefCell::new(bytes)))
            }
            SendValue::Bytes(bytes) => alloc(HeapObject::LBytes(bytes)),
            SendValue::Blob(bytes) => alloc(HeapObject::LBytesMut(std::cell::RefCell::new(bytes))),
            SendValue::LBox(contents, is_local) => {
                let val = contents.into_value();
                // Preserve the lbox type (local vs user) across thread boundary
                alloc(HeapObject::LBox(std::cell::RefCell::new(val), is_local))
            }
            SendValue::Float(f) => alloc(HeapObject::Float(f)),
            SendValue::FFIType(desc) => alloc(HeapObject::FFIType(desc)),
            SendValue::LSet(items) => {
                let set: BTreeSet<Value> = items.into_iter().map(|sv| sv.into_value()).collect();
                alloc(HeapObject::LSet(set))
            }
            SendValue::LSetMut(items) => {
                let set: BTreeSet<Value> = items.into_iter().map(|sv| sv.into_value()).collect();
                alloc(HeapObject::LSetMut(std::cell::RefCell::new(set)))
            }
            SendValue::NativeFn(f) => Value::native_fn(f),
            SendValue::Closure(_) => {
                panic!("bug: bare SendValue::Closure; use SendBundle::into_value")
            }
            SendValue::Ref(_) => panic!("bug: bare SendValue::Ref; use SendBundle::into_value"),
        }
    }
}

// SAFETY: SendValue is safe to send because it owns all its data
unsafe impl Send for SendValue {}
unsafe impl Sync for SendValue {}

impl SendBundle {
    /// Serialize a `Value` into a `SendBundle`.
    ///
    /// Closures — including mutually recursive ones — are placed in the intern
    /// table and referenced by index via `SendValue::Ref`. The root `SendValue`
    /// may itself be a `Ref(0)` if `value` is a closure.
    ///
    /// Returns `Err` if any value in the reachable graph is not sendable
    /// (e.g., mutable @struct, fiber, FFI handle).
    pub fn from_value(value: Value) -> Result<Self, String> {
        let mut ctx = SerContext {
            visited: HashMap::new(),
            closures: Vec::new(),
        };
        let root = from_value_inner(value, &mut ctx)?;
        Ok(SendBundle {
            root,
            closures: ctx.closures,
        })
    }
}
