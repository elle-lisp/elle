//! SendValue wrapper for thread-safe value transmission
//!
//! This module provides SendValue, a wrapper around Value that implements Send
//! by deep-copying heap values instead of sharing raw pointers.
//!
//! The problem with raw Value copies: Value contains raw pointers to Rc
//! heap objects. When sent to another thread, the original Rc may drop and free the
//! heap object while the thread still holds a raw pointer to it.
//!
//! The solution: SendValue stores owned copies of heap data, not raw pointers.

use super::heap::{alloc, deref, Cons, HeapObject};
use super::repr::Value;
use crate::error::LocationMap;
use crate::hir::VarargKind;
use crate::signals::Signal;
use crate::value::fiber::SignalBits;
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
    pub signal: Signal,
    pub lbox_params_mask: u64,
    pub lbox_locals_mask: u64,
    pub symbol_names: HashMap<u32, String>,
    pub location_map: LocationMap,
    pub doc: Option<Box<SendValue>>,
    pub vararg_kind: VarargKind,
    pub name: Option<String>,
    pub squelch_mask: SignalBits,
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

    /// Deep copy of cons cells (with traits)
    Cons(Box<SendValue>, Box<SendValue>, Box<SendValue>),

    /// Deep copy of arrays (with traits)
    Array(Vec<SendValue>, Box<SendValue>),

    /// Deep copy of structs (immutable maps, with traits)
    Struct(
        BTreeMap<crate::value::heap::TableKey, SendValue>,
        Box<SendValue>,
    ),

    /// Deep copy of arrays (immutable fixed-length sequences, with traits)
    Tuple(Vec<SendValue>, Box<SendValue>),

    /// Deep copy of @strings (mutable byte sequences, with traits)
    Buffer(Vec<u8>, Box<SendValue>),

    /// Deep copy of @bytes (immutable binary data, with traits)
    Bytes(Vec<u8>, Box<SendValue>),

    /// Deep copy of @bytes (mutable binary data, with traits)
    Blob(Vec<u8>, Box<SendValue>),

    /// Deep copy of mutable boxes (if contents are sendable)
    /// The bool indicates if it's a local lbox (auto-unwrapped) or user box
    /// Third field is the traits
    LBox(Box<SendValue>, bool, Box<SendValue>),

    /// Float values that couldn't be stored inline
    Float(f64),

    /// Deep copy of FFI type descriptor (pure data, no Rc)
    FFIType(crate::ffi::types::TypeDesc),

    /// Deep copy of immutable sets (with traits)
    LSet(Vec<SendValue>, Box<SendValue>),

    /// Deep copy of mutable sets (with traits)
    LSetMut(Vec<SendValue>, Box<SendValue>),

    /// Native function pointer (inherently Send + Sync)
    NativeFn(crate::value::types::NativeFn),

    /// Deep copy of a closure (template + captured environment).
    /// Only appears as an entry in `SendBundle::closures`.
    /// The root `SendValue` tree and closure envs reference closures via `Ref(idx)`.
    Closure(Box<SendableClosure>),

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
    /// Maps `value.payload` (heap pointer address) → intern table index.
    /// Inserted BEFORE recursing into a closure's fields, so back-references find it.
    visited: HashMap<u64, usize>,
    /// Intern table being built.
    closures: Vec<SendableClosure>,
}

/// Recursive worker for serialization. Threads SerContext through all recursive calls.
fn from_value_inner(value: Value, ctx: &mut SerContext) -> Result<SendValue, String> {
    // Keywords carry their name for cross-thread re-interning
    if let Some(name) = value.as_keyword_name() {
        return Ok(SendValue::Keyword(name));
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
        HeapObject::LString { s, .. } => Ok(SendValue::String(s.to_string())),

        // Cons cells - deep copy both first and rest, plus traits
        HeapObject::Cons(cons) => {
            let first = from_value_inner(cons.first, ctx)?;
            let rest = from_value_inner(cons.rest, ctx)?;
            let traits = from_value_inner(cons.traits, ctx)?;
            Ok(SendValue::Cons(
                Box::new(first),
                Box::new(rest),
                Box::new(traits),
            ))
        }

        // Arrays - deep copy all elements, plus traits
        HeapObject::LArrayMut {
            data: vec_ref,
            traits,
            ..
        } => {
            let borrowed = vec_ref
                .try_borrow()
                .map_err(|_| "Cannot borrow array for sending".to_string())?;
            let copied: Result<Vec<SendValue>, String> =
                borrowed.iter().map(|v| from_value_inner(*v, ctx)).collect();
            let traits_sv = from_value_inner(*traits, ctx)?;
            Ok(SendValue::Array(copied?, Box::new(traits_sv)))
        }

        // Structs - deep copy all values, plus traits
        HeapObject::LStruct {
            data: s, traits, ..
        } => {
            let mut copied = BTreeMap::new();
            for (k, v) in s.iter() {
                if !k.is_sendable() {
                    return Err("Cannot send struct with identity keys".to_string());
                }
                copied.insert(k.clone(), from_value_inner(*v, ctx)?);
            }
            let traits_sv = from_value_inner(*traits, ctx)?;
            Ok(SendValue::Struct(copied, Box::new(traits_sv)))
        }

        // Arrays (immutable) - deep copy all elements, plus traits
        HeapObject::LArray {
            elements: elems,
            traits,
            ..
        } => {
            let copied: Result<Vec<SendValue>, String> =
                elems.iter().map(|v| from_value_inner(*v, ctx)).collect();
            let traits_sv = from_value_inner(*traits, ctx)?;
            Ok(SendValue::Tuple(copied?, Box::new(traits_sv)))
        }

        // @string - deep copy the bytes, plus traits
        HeapObject::LStringMut {
            data: buf_ref,
            traits,
            ..
        } => {
            let borrowed = buf_ref
                .try_borrow()
                .map_err(|_| "Cannot borrow @string for sending".to_string())?;
            let traits_sv = from_value_inner(*traits, ctx)?;
            Ok(SendValue::Buffer(borrowed.clone(), Box::new(traits_sv)))
        }

        // Boxes - deep copy the contents if sendable, plus traits
        HeapObject::LBox {
            cell: cell_ref,
            is_local,
            traits,
            ..
        } => {
            let borrowed = cell_ref
                .try_borrow()
                .map_err(|_| "Cannot borrow box for sending".to_string())?;
            let contents = from_value_inner(*borrowed, ctx)?;
            let traits_sv = from_value_inner(*traits, ctx)?;
            Ok(SendValue::LBox(
                Box::new(contents),
                *is_local,
                Box::new(traits_sv),
            ))
        }

        // Float values that couldn't be stored inline
        HeapObject::Float(f) => Ok(SendValue::Float(*f)),

        // Unsafe: mutable @structs
        HeapObject::LStructMut { .. } => Err("Cannot send mutable @struct".to_string()),

        // Closures: intern into the table, with cycle detection via pre-insertion
        HeapObject::Closure {
            closure: closure_rc,
            traits: _,
        } => {
            // Use value.payload as identity key — for heap values, payload IS the pointer.
            let key = value.payload;

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
                signal: closure_rc.template.signal,
                lbox_params_mask: 0,
                lbox_locals_mask: 0,
                symbol_names: HashMap::new(),
                location_map: LocationMap::new(),
                doc: None,
                vararg_kind: closure_rc.template.vararg_kind.clone(),
                name: None,
                squelch_mask: SignalBits::EMPTY,
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
                signal: closure_rc.template.signal,
                lbox_params_mask: closure_rc.template.lbox_params_mask,
                lbox_locals_mask: closure_rc.template.lbox_locals_mask,
                symbol_names: (*closure_rc.template.symbol_names).clone(),
                location_map: (*closure_rc.template.location_map).clone(),
                doc,
                vararg_kind: closure_rc.template.vararg_kind.clone(),
                name: closure_rc.template.name.as_ref().map(|s| s.to_string()),
                squelch_mask: closure_rc.squelch_mask,
                env,
            };

            Ok(SendValue::Ref(idx))
        }

        // Native function pointers are inherently Send + Sync
        HeapObject::NativeFn(f) => Ok(SendValue::NativeFn(*f)),

        // Unsafe: FFI handles
        HeapObject::LibHandle(_) => Err("Cannot send library handle".to_string()),

        // Unsafe: thread handles
        HeapObject::ThreadHandle { .. } => Err("Cannot send thread handle".to_string()),

        // Unsafe: fibers (contain execution state with closures)
        HeapObject::Fiber { .. } => Err("Cannot send fiber".to_string()),

        // Unsafe: syntax objects (contain Rc)
        HeapObject::Syntax { .. } => Err("Cannot send syntax object".to_string()),

        // Unsafe: FFI signatures (contain non-Send types like Cif)
        HeapObject::FFISignature(_, _) => Err("Cannot send FFI signature".to_string()),

        // Unsafe: managed pointers (lifecycle state is not thread-safe with Cell)
        HeapObject::ManagedPointer { .. } => Err("Cannot send managed pointer".to_string()),

        // Unsafe: external objects (contain Rc<dyn Any>, not thread-safe)
        HeapObject::External { .. } => Err("Cannot send external object".to_string()),

        // Unsafe: parameters (fiber-local state)
        HeapObject::Parameter { .. } => Err("Cannot send parameter".to_string()),

        // FFI type descriptors are pure data — safe to send
        HeapObject::FFIType(desc) => Ok(SendValue::FFIType(desc.clone())),

        // Bytes - immutable and safe to send, plus traits
        HeapObject::LBytes {
            data: b, traits, ..
        } => {
            let traits_sv = from_value_inner(*traits, ctx)?;
            Ok(SendValue::Bytes(b.clone(), Box::new(traits_sv)))
        }

        // @bytes - deep copy the bytes, plus traits
        HeapObject::LBytesMut {
            data: blob_ref,
            traits,
            ..
        } => {
            let borrowed = blob_ref
                .try_borrow()
                .map_err(|_| "Cannot borrow @bytes for sending".to_string())?;
            let traits_sv = from_value_inner(*traits, ctx)?;
            Ok(SendValue::Blob(borrowed.clone(), Box::new(traits_sv)))
        }

        // Sets (immutable) - deep copy all elements, plus traits
        HeapObject::LSet {
            data: s, traits, ..
        } => {
            let copied: Result<Vec<SendValue>, String> =
                s.iter().map(|v| from_value_inner(*v, ctx)).collect();
            let traits_sv = from_value_inner(*traits, ctx)?;
            Ok(SendValue::LSet(copied?, Box::new(traits_sv)))
        }

        // Sets (mutable) - deep copy all elements, plus traits
        HeapObject::LSetMut {
            data: s_ref,
            traits,
            ..
        } => {
            let borrowed = s_ref
                .try_borrow()
                .map_err(|_| "Cannot borrow mutable set for sending".to_string())?;
            let copied: Result<Vec<SendValue>, String> =
                borrowed.iter().map(|v| from_value_inner(*v, ctx)).collect();
            let traits_sv = from_value_inner(*traits, ctx)?;
            Ok(SendValue::LSetMut(copied?, Box::new(traits_sv)))
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
        if !ctx.closures.is_empty() {
            panic!("SendValue::from_value cannot serialize closures; use SendBundle::from_value instead");
        }
        Ok(sv)
    }

    /// Convert SendValue back into a Value by reconstructing heap objects.
    pub fn into_value(self) -> Value {
        match self {
            SendValue::Immediate(v) => v,
            SendValue::Keyword(name) => Value::keyword(&name),
            SendValue::String(s) => Value::string_no_intern(s),
            SendValue::Cons(first, rest, traits) => {
                let first_val = first.into_value();
                let rest_val = rest.into_value();
                let traits_val = traits.into_value();
                let cons = Cons {
                    first: first_val,
                    rest: rest_val,
                    traits: traits_val,
                };
                alloc(HeapObject::Cons(cons))
            }
            SendValue::Array(items, traits) => {
                let values: Vec<Value> = items.into_iter().map(|sv| sv.into_value()).collect();
                let traits_val = traits.into_value();
                alloc(HeapObject::LArrayMut {
                    data: std::cell::RefCell::new(values),
                    traits: traits_val,
                })
            }
            SendValue::Struct(map, traits) => {
                let values: BTreeMap<_, _> = map
                    .into_iter()
                    .map(|(k, sv)| (k, sv.into_value()))
                    .collect();
                let traits_val = traits.into_value();
                alloc(HeapObject::LStruct {
                    data: values,
                    traits: traits_val,
                })
            }
            SendValue::Tuple(items, traits) => {
                let values: Vec<Value> = items.into_iter().map(|sv| sv.into_value()).collect();
                let traits_val = traits.into_value();
                alloc(HeapObject::LArray {
                    elements: values,
                    traits: traits_val,
                })
            }
            SendValue::Buffer(bytes, traits) => {
                let traits_val = traits.into_value();
                alloc(HeapObject::LStringMut {
                    data: std::cell::RefCell::new(bytes),
                    traits: traits_val,
                })
            }
            SendValue::Bytes(bytes, traits) => {
                let traits_val = traits.into_value();
                alloc(HeapObject::LBytes {
                    data: bytes,
                    traits: traits_val,
                })
            }
            SendValue::Blob(bytes, traits) => {
                let traits_val = traits.into_value();
                alloc(HeapObject::LBytesMut {
                    data: std::cell::RefCell::new(bytes),
                    traits: traits_val,
                })
            }
            SendValue::LBox(contents, is_local, traits) => {
                let val = contents.into_value();
                let traits_val = traits.into_value();
                // Preserve the lbox type (local vs user) across thread boundary
                alloc(HeapObject::LBox {
                    cell: std::cell::RefCell::new(val),
                    is_local,
                    traits: traits_val,
                })
            }
            SendValue::Float(f) => alloc(HeapObject::Float(f)),
            SendValue::FFIType(desc) => alloc(HeapObject::FFIType(desc)),
            SendValue::LSet(items, traits) => {
                let set: BTreeSet<Value> = items.into_iter().map(|sv| sv.into_value()).collect();
                let traits_val = traits.into_value();
                alloc(HeapObject::LSet {
                    data: set,
                    traits: traits_val,
                })
            }
            SendValue::LSetMut(items, traits) => {
                let set: BTreeSet<Value> = items.into_iter().map(|sv| sv.into_value()).collect();
                let traits_val = traits.into_value();
                alloc(HeapObject::LSetMut {
                    data: std::cell::RefCell::new(set),
                    traits: traits_val,
                })
            }
            SendValue::NativeFn(f) => Value::native_fn(f),
            SendValue::Closure(_box_val) => {
                panic!("bug: bare SendValue::Closure; use SendBundle::into_value")
            }
            SendValue::Ref(_) => panic!("bug: bare SendValue::Ref; use SendBundle::into_value"),
        }
    }
}

// SAFETY: SendValue is safe to send because it owns all its data
unsafe impl Send for SendValue {}
unsafe impl Sync for SendValue {}

/// Reconstruction state for a single intern table entry.
enum ReconState {
    NotStarted,
    InProgress,
    Done(Value),
}

/// Per-call deserialization context for `SendBundle::into_value`.
struct DeserContext {
    /// Owned closure data. Entries are `take`n as they are reconstructed.
    closures: Vec<Option<SendableClosure>>,
    /// Reconstruction state per intern table index.
    states: Vec<ReconState>,
    /// Deferred fixups: (LBox Value that holds a NIL placeholder, intern index).
    /// After all closures are built, each LBox's RefCell is overwritten with
    /// the actual closure value.
    fixups: Vec<(Value, usize)>,
}

/// Recursive worker for deserialization. Threads DeserContext through all recursive calls.
fn into_value_inner(sv: SendValue, ctx: &mut DeserContext) -> Value {
    use crate::value::closure::{Closure, ClosureTemplate};
    use crate::value::heap::{alloc, Cons, HeapObject};
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use std::rc::Rc;

    match sv {
        SendValue::Immediate(v) => v,
        SendValue::Keyword(name) => Value::keyword(&name),
        SendValue::String(s) => Value::string_no_intern(s),
        SendValue::Cons(first, rest, traits) => {
            let f = into_value_inner(*first, ctx);
            let r = into_value_inner(*rest, ctx);
            let t = into_value_inner(*traits, ctx);
            alloc(HeapObject::Cons(Cons {
                first: f,
                rest: r,
                traits: t,
            }))
        }
        SendValue::Array(items, traits) => {
            let values: Vec<Value> = items
                .into_iter()
                .map(|sv| into_value_inner(sv, ctx))
                .collect();
            let traits_val = into_value_inner(*traits, ctx);
            alloc(HeapObject::LArrayMut {
                data: RefCell::new(values),
                traits: traits_val,
            })
        }
        SendValue::Struct(map, traits) => {
            let values: std::collections::BTreeMap<_, _> = map
                .into_iter()
                .map(|(k, sv)| (k, into_value_inner(sv, ctx)))
                .collect();
            let traits_val = into_value_inner(*traits, ctx);
            alloc(HeapObject::LStruct {
                data: values,
                traits: traits_val,
            })
        }
        SendValue::Tuple(items, traits) => {
            let values: Vec<Value> = items
                .into_iter()
                .map(|sv| into_value_inner(sv, ctx))
                .collect();
            let traits_val = into_value_inner(*traits, ctx);
            alloc(HeapObject::LArray {
                elements: values,
                traits: traits_val,
            })
        }
        SendValue::Buffer(bytes, traits) => {
            let traits_val = into_value_inner(*traits, ctx);
            alloc(HeapObject::LStringMut {
                data: RefCell::new(bytes),
                traits: traits_val,
            })
        }
        SendValue::Bytes(bytes, traits) => {
            let traits_val = into_value_inner(*traits, ctx);
            alloc(HeapObject::LBytes {
                data: bytes,
                traits: traits_val,
            })
        }
        SendValue::Blob(bytes, traits) => {
            let traits_val = into_value_inner(*traits, ctx);
            alloc(HeapObject::LBytesMut {
                data: RefCell::new(bytes),
                traits: traits_val,
            })
        }

        SendValue::LBox(contents, is_local, traits) => {
            // Special case: if contents is a Ref to an InProgress closure,
            // emit NIL placeholder and record a fixup.
            let fixup_idx = match *contents {
                SendValue::Ref(idx) => {
                    if matches!(ctx.states[idx], ReconState::InProgress) {
                        Some(idx)
                    } else {
                        None
                    }
                }
                _ => None,
            };
            let inner_val = into_value_inner(*contents, ctx);
            let traits_val = into_value_inner(*traits, ctx);
            let lbox_val = alloc(HeapObject::LBox {
                cell: RefCell::new(inner_val),
                is_local,
                traits: traits_val,
            });
            if let Some(idx) = fixup_idx {
                ctx.fixups.push((lbox_val, idx));
            }
            lbox_val
        }

        SendValue::Float(f) => alloc(HeapObject::Float(f)),
        SendValue::FFIType(desc) => alloc(HeapObject::FFIType(desc)),
        SendValue::LSet(items, traits) => {
            let set: BTreeSet<Value> = items
                .into_iter()
                .map(|sv| into_value_inner(sv, ctx))
                .collect();
            let traits_val = into_value_inner(*traits, ctx);
            alloc(HeapObject::LSet {
                data: set,
                traits: traits_val,
            })
        }
        SendValue::LSetMut(items, traits) => {
            let set: BTreeSet<Value> = items
                .into_iter()
                .map(|sv| into_value_inner(sv, ctx))
                .collect();
            let traits_val = into_value_inner(*traits, ctx);
            alloc(HeapObject::LSetMut {
                data: RefCell::new(set),
                traits: traits_val,
            })
        }
        SendValue::NativeFn(f) => Value::native_fn(f),

        // Closure variant: only appears stored directly in SendBundle::closures.
        // At the top-level call it means the bundle was constructed incorrectly.
        // In practice root is always a Ref when the value is a closure.
        SendValue::Closure(_box_val) => panic!("bug: bare Closure in SendValue tree; use Ref"),

        SendValue::Ref(idx) => {
            if let ReconState::Done(v) = ctx.states[idx] {
                return v;
            }
            if matches!(ctx.states[idx], ReconState::InProgress) {
                return Value::NIL; // placeholder; fixup will correct it
            }
            // NotStarted — fall through to reconstruct

            ctx.states[idx] = ReconState::InProgress;
            let sc = ctx.closures[idx]
                .take()
                .expect("bug: closure already taken from DeserContext");

            // Reconstruct constants (no closures expected in constants,
            // but thread the context for completeness).
            let constants: Vec<Value> = sc
                .constants
                .into_iter()
                .map(|sv| into_value_inner(sv, ctx))
                .collect();

            // Reconstruct env (may encounter InProgress Refs → NIL placeholders).
            let env: Vec<Value> = sc
                .env
                .into_iter()
                .map(|sv| into_value_inner(sv, ctx))
                .collect();

            let doc = sc.doc.map(|sv| into_value_inner(*sv, ctx));

            let template = Rc::new(ClosureTemplate {
                bytecode: Rc::new(sc.bytecode),
                arity: sc.arity,
                num_locals: sc.num_locals,
                num_captures: sc.num_captures,
                num_params: sc.num_params,
                constants: Rc::new(constants),
                signal: sc.signal,
                lbox_params_mask: sc.lbox_params_mask,
                lbox_locals_mask: sc.lbox_locals_mask,
                symbol_names: Rc::new(sc.symbol_names),
                location_map: Rc::new(sc.location_map),
                jit_code: None,
                lir_function: None,
                doc,
                syntax: None,
                vararg_kind: sc.vararg_kind,
                name: sc.name.map(|s| Rc::from(s.as_str())),
                result_is_immediate: false,
                has_outward_heap_set: false,
                wasm_func_idx: None,
            });

            let val = Value::closure(Closure {
                template,
                env: Rc::new(env),
                squelch_mask: sc.squelch_mask,
            });
            ctx.states[idx] = ReconState::Done(val);
            val
        }
    }
}

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

    /// Reconstruct a `Value` from this bundle.
    ///
    /// Mutually recursive closures are handled via LBox fixups: if a closure's
    /// env contains an LBox wrapping a not-yet-built closure, the LBox is
    /// allocated with a NIL placeholder and updated after all closures are built.
    pub fn into_value(self) -> Value {
        let n = self.closures.len();
        let mut ctx = DeserContext {
            closures: self.closures.into_iter().map(Some).collect(),
            states: (0..n).map(|_| ReconState::NotStarted).collect(),
            fixups: Vec::new(),
        };

        let result = into_value_inner(self.root, &mut ctx);

        // Fixup pass: patch LBox cells that were given NIL placeholders.
        for (lbox_val, idx) in &ctx.fixups {
            let closure_val = match ctx.states[*idx] {
                ReconState::Done(v) => v,
                _ => panic!(
                    "bug: fixup references closure that was never built (idx={})",
                    idx
                ),
            };
            if let Some(cell) = lbox_val.as_lbox() {
                *cell.borrow_mut() = closure_val;
            }
        }

        result
    }
}
