//! Serialization of a live `Value` into a Send-safe `SendValue`.
//!
//! The module root holds the value-tag dispatch (`from_value_inner`) and the
//! traits-field helper it leans on. Cohesive sub-concerns live in siblings:
//! `ctx` (the threaded bookkeeping context), `lir` (LIR-for-send rewriting),
//! `template` (closure blueprint copies), and `closure` (the interning closure
//! arm). Re-exports keep `ser::from_value_inner` / `ser::SerContext` resolving
//! for the parent module unchanged.

use super::syntax::syntax_to_send;
use super::*;

mod closure;
mod ctx;
mod lir;
mod template;

pub(super) use ctx::SerContext;
pub(super) use template::sendable_from_template;

use closure::send_closure;

/// Send a traits field. Default traitsets (from the registry) are
/// skipped (sent as NIL) since the receiving thread has its own registry.
/// User-attached traits are deep-copied normally.
fn send_traits(traits: Value, tag: HeapTag, ctx: &mut SerContext<'_>) -> Result<SendValue, String> {
    if traits.is_nil() {
        return Ok(SendValue::Immediate(Value::NIL));
    }
    // Check pointer identity against the registry default for this tag, on the
    // SENDER's heap (the value being serialized lives there). This distinguishes
    // registry defaults (skip) from user-attached @struct traits (send faithfully).
    let default = crate::primitives::traitregistry::default_traits_for(ctx.heap, tag);
    if !default.is_nil() && traits.payload == default.payload {
        return Ok(SendValue::Immediate(Value::NIL));
    }
    // User-attached traits — send normally
    from_value_inner(traits, ctx)
}

/// Recursive worker for serialization. Threads SerContext through all recursive calls.
pub(super) fn from_value_inner(
    value: Value,
    ctx: &mut SerContext<'_>,
) -> Result<SendValue, String> {
    // Keywords carry their name for cross-thread re-interning
    if let Some(name) = value.as_keyword_name() {
        return Ok(SendValue::Keyword(name));
    }

    // Symbols carry their name for cross-thread re-interning (IDs are
    // per-table). If the id is not in the sender's table (should not happen),
    // fall through to Immediate.
    if let Some(id) = value.as_symbol() {
        if let Some(name) = ctx.symbols.name(crate::value::SymbolId(id)) {
            return Ok(SendValue::Symbol {
                name: name.to_string(),
                id,
            });
        }
        return Ok(SendValue::Immediate(value));
    }

    // Immediate values are always safe
    if value.is_nil() || value.is_bool() || value.is_int() || value.is_float() {
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
        HeapObject::LString { s, .. } => Ok(SendValue::String(unsafe {
            std::str::from_utf8_unchecked(s.as_slice()).to_string()
        })),

        // Pair cells - deep copy both first and rest, plus traits
        HeapObject::Pair(pair) => {
            let first = from_value_inner(pair.first, ctx)?;
            let rest = from_value_inner(pair.rest, ctx)?;
            let traits = send_traits(pair.traits, HeapTag::Pair, ctx)?;
            Ok(SendValue::Pair(
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
            let traits_sv = send_traits(*traits, HeapTag::LArrayMut, ctx)?;
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
            let traits_sv = send_traits(*traits, HeapTag::LStruct, ctx)?;
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
            let traits_sv = send_traits(*traits, HeapTag::LArray, ctx)?;
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
            let traits_sv = send_traits(*traits, HeapTag::LStringMut, ctx)?;
            Ok(SendValue::Buffer(borrowed.clone(), Box::new(traits_sv)))
        }

        // User boxes - deep copy the contents if sendable, plus traits
        HeapObject::LBox {
            cell: cell_ref,
            traits,
            ..
        } => {
            let borrowed = cell_ref
                .try_borrow()
                .map_err(|_| "Cannot borrow box for sending".to_string())?;
            let contents = from_value_inner(*borrowed, ctx)?;
            let traits_sv = send_traits(*traits, HeapTag::LBox, ctx)?;
            Ok(SendValue::LBox(Box::new(contents), Box::new(traits_sv)))
        }

        // Compiler capture cells - deep copy the contents if sendable, plus traits
        HeapObject::CaptureCell {
            cell: cell_ref,
            traits,
            ..
        } => {
            let borrowed = cell_ref
                .try_borrow()
                .map_err(|_| "Cannot borrow capture cell for sending".to_string())?;
            let contents = from_value_inner(*borrowed, ctx)?;
            let traits_sv = send_traits(*traits, HeapTag::CaptureCell, ctx)?;
            Ok(SendValue::CaptureCell(
                Box::new(contents),
                Box::new(traits_sv),
            ))
        }

        // Float values that couldn't be stored inline
        HeapObject::Float(f) => Ok(SendValue::Float(*f)),

        // Mutable @structs — deep copy all values, plus traits
        HeapObject::LStructMut {
            data: map_ref,
            traits,
            ..
        } => {
            let borrowed = map_ref
                .try_borrow()
                .map_err(|_| "Cannot borrow @struct for sending".to_string())?;
            let mut copied = BTreeMap::new();
            for (k, v) in borrowed.iter() {
                if !k.is_sendable() {
                    return Err("Cannot send @struct with identity keys".to_string());
                }
                copied.insert(k.clone(), from_value_inner(*v, ctx)?);
            }
            let traits_sv = send_traits(*traits, HeapTag::LStructMut, ctx)?;
            Ok(SendValue::StructMut(copied, Box::new(traits_sv)))
        }

        // Closures: intern into the table, with cycle detection via pre-insertion
        HeapObject::Closure {
            closure: closure_rc,
            traits: _,
        } => send_closure(value, closure_rc, ctx),

        // (Native-fns are immediates — `Value{TAG_NATIVE_FN, prim_id}` — and
        // serialize via the `Immediate` arm above. The prim_id is stable across
        // threads/processes via deterministic registration, so it re-resolves to
        // the same primitive on the receiver. They never reach this heap match.)

        // Unsafe: FFI handles
        HeapObject::LibHandle(_) => Err("Cannot send library handle".to_string()),

        // Unsafe: thread handles
        HeapObject::ThreadHandle { .. } => Err("Cannot send thread handle".to_string()),

        // Unsafe: fibers (contain execution state with closures)
        HeapObject::Fiber { .. } => Err("Cannot send fiber".to_string()),

        // Parsed syntax: serialized to a self-contained Send-safe mirror.
        HeapObject::Syntax { syntax, .. } => {
            Ok(SendValue::Syntax(Box::new(syntax_to_send(syntax)?)))
        }

        // Unsafe: FFI signatures (contain non-Send types like Cif)
        HeapObject::FFISignature(_, _) => Err("Cannot send FFI signature".to_string()),

        // Unsafe: managed pointers (lifecycle state is not thread-safe with Cell)
        HeapObject::ManagedPointer { .. } => Err("Cannot send managed pointer".to_string()),

        // External objects: channels and stdio ports are sendable, others not.
        HeapObject::External { obj, .. } => match obj.type_name {
            "chan/sender" => crate::primitives::chan::clone_sender(&value)
                .map(|(tx, wake)| SendValue::ChanSender(tx, wake))
                .ok_or_else(|| "Cannot send closed channel sender".to_string()),
            "chan/receiver" => crate::primitives::chan::clone_receiver(&value)
                .map(|(rx, wake)| SendValue::ChanReceiver(rx, wake))
                .ok_or_else(|| "Cannot send closed channel receiver".to_string()),
            // Stdin/Stdout/Stderr ports carry no owned fd — reconstruct fresh in
            // the worker. File/socket ports own their fd and are not sendable.
            "port" => {
                use crate::port::{Port, PortKind};
                match value.as_external::<Port>().map(|p| p.kind()) {
                    Some(k @ (PortKind::Stdin | PortKind::Stdout | PortKind::Stderr)) => {
                        Ok(SendValue::StdioPort(k))
                    }
                    Some(_) => Err(
                        "Cannot send a file or socket port (only stdin/stdout/stderr)".to_string(),
                    ),
                    None => Err("Cannot send port: not a port object".to_string()),
                }
            }
            _ => Err(format!("Cannot send external object: {}", obj.type_name)),
        },

        // Parameters: sendable iff their default + traits are. The id is
        // preserved (resolution is by id), so the worker resolves the same
        // parameter the originating closure closed over.
        HeapObject::Parameter {
            id,
            default,
            traits,
        } => {
            let d = from_value_inner(*default, ctx)?;
            let t = from_value_inner(*traits, ctx)?;
            Ok(SendValue::Parameter {
                id: *id,
                default: Box::new(d),
                traits: Box::new(t),
            })
        }

        // FFI type descriptors are pure data — safe to send
        HeapObject::FFIType(desc) => Ok(SendValue::FFIType(desc.clone())),

        // Bytes - immutable and safe to send, plus traits
        HeapObject::LBytes {
            data: b, traits, ..
        } => {
            let traits_sv = send_traits(*traits, HeapTag::LBytes, ctx)?;
            Ok(SendValue::Bytes(b.as_slice().to_vec(), Box::new(traits_sv)))
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
            let traits_sv = send_traits(*traits, HeapTag::LBytesMut, ctx)?;
            Ok(SendValue::Blob(borrowed.clone(), Box::new(traits_sv)))
        }

        // Sets (immutable) - deep copy all elements, plus traits
        HeapObject::LSet {
            data: s, traits, ..
        } => {
            let copied: Result<Vec<SendValue>, String> =
                s.iter().map(|v| from_value_inner(*v, ctx)).collect();
            let traits_sv = send_traits(*traits, HeapTag::LSet, ctx)?;
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
            let traits_sv = send_traits(*traits, HeapTag::LSetMut, ctx)?;
            Ok(SendValue::LSetMut(copied?, Box::new(traits_sv)))
        }

        // A bare closure template is never a top-level user value (it is reached
        // only as a closure instance's `Region` template, serialized via the
        // Closure arm's `child_protos`), so it is never sent on its own.
        HeapObject::ClosureTemplate(_) => Err("Cannot send a bare closure template".to_string()),
    }
}
