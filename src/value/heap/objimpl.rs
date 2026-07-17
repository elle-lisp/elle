use super::*;

impl HeapObject {
    /// Get the type tag for this heap object.
    #[inline]
    pub fn tag(&self) -> HeapTag {
        match self {
            HeapObject::LString { .. } => HeapTag::LString,
            HeapObject::Pair(_) => HeapTag::Pair,
            HeapObject::LArrayMut { .. } => HeapTag::LArrayMut,
            HeapObject::LStructMut { .. } => HeapTag::LStructMut,
            HeapObject::LStruct { .. } => HeapTag::LStruct,
            HeapObject::Closure { .. } => HeapTag::Closure,
            HeapObject::LArray { .. } => HeapTag::LArray,
            HeapObject::LStringMut { .. } => HeapTag::LStringMut,
            HeapObject::LBytes { .. } => HeapTag::LBytes,
            HeapObject::LBytesMut { .. } => HeapTag::LBytesMut,
            HeapObject::LBox { .. } => HeapTag::LBox,
            HeapObject::CaptureCell { .. } => HeapTag::CaptureCell,
            HeapObject::Float(_) => HeapTag::Float,
            HeapObject::LibHandle(_) => HeapTag::LibHandle,
            HeapObject::ThreadHandle { .. } => HeapTag::ThreadHandle,
            HeapObject::Fiber { .. } => HeapTag::Fiber,
            HeapObject::Syntax { .. } => HeapTag::Syntax,
            HeapObject::FFISignature(_, _) => HeapTag::FFISignature,
            HeapObject::FFIType(_) => HeapTag::FFIType,
            HeapObject::ManagedPointer { .. } => HeapTag::ManagedPointer,
            HeapObject::External { .. } => HeapTag::External,
            HeapObject::Parameter { .. } => HeapTag::Parameter,
            HeapObject::LSet { .. } => HeapTag::LSet,
            HeapObject::LSetMut { .. } => HeapTag::LSetMut,
            HeapObject::ClosureTemplate(_) => HeapTag::ClosureTemplate,
        }
    }

    /// Read the trait table attached to this object, or `Value::NIL` if it has
    /// none (the 5 infrastructure variants, or a never-traited object). This is
    /// the single read site for the `traits` side-field: `traitregistry::
    /// get_traitset` (the `Value` entry point) and the region cross-ref scan
    /// (`find_object_cross_refs`) both go through it, so the field is enumerated
    /// in exactly one place — the trait table is a cross-region edge like any
    /// other content field and must be RC-tracked symmetrically (Rule 5/7).
    #[inline]
    pub fn traits(&self) -> Value {
        match self {
            HeapObject::LString { traits, .. }
            | HeapObject::LArray { traits, .. }
            | HeapObject::LArrayMut { traits, .. }
            | HeapObject::LStruct { traits, .. }
            | HeapObject::LStructMut { traits, .. }
            | HeapObject::LStringMut { traits, .. }
            | HeapObject::LBytes { traits, .. }
            | HeapObject::LBytesMut { traits, .. }
            | HeapObject::LSet { traits, .. }
            | HeapObject::LSetMut { traits, .. }
            | HeapObject::Closure { traits, .. }
            | HeapObject::LBox { traits, .. }
            | HeapObject::CaptureCell { traits, .. }
            | HeapObject::Fiber { traits, .. }
            | HeapObject::Syntax { traits, .. }
            | HeapObject::ManagedPointer { traits, .. }
            | HeapObject::External { traits, .. }
            | HeapObject::Parameter { traits, .. }
            | HeapObject::ThreadHandle { traits, .. } => *traits,
            HeapObject::Pair(pair) => pair.traits,
            HeapObject::Float(_)
            | HeapObject::LibHandle(_)
            | HeapObject::FFISignature(_, _)
            | HeapObject::FFIType(_)
            | HeapObject::ClosureTemplate(_) => Value::NIL,
        }
    }

    /// Get the Value-level TAG_* constant for this heap object.
    /// Used by the allocator to stamp the tag into the returned Value.
    #[inline]
    pub fn value_tag(&self) -> u64 {
        use crate::value::repr::{
            TAG_ARRAY, TAG_ARRAY_MUT, TAG_BYTES, TAG_BYTES_MUT, TAG_CAPTURE_CELL, TAG_CLOSURE,
            TAG_CLOSURE_TEMPLATE, TAG_CONS, TAG_EXTERNAL, TAG_FFI_SIG, TAG_FFI_TYPE, TAG_FIBER,
            TAG_LBOX, TAG_LIB_HANDLE, TAG_MANAGED_PTR, TAG_PARAMETER, TAG_SET, TAG_SET_MUT,
            TAG_STRING, TAG_STRING_MUT, TAG_STRUCT, TAG_STRUCT_MUT, TAG_SYNTAX, TAG_THREAD,
        };
        match self {
            HeapObject::LString { .. } => TAG_STRING,
            HeapObject::LStringMut { .. } => TAG_STRING_MUT,
            HeapObject::LArray { .. } => TAG_ARRAY,
            HeapObject::LArrayMut { .. } => TAG_ARRAY_MUT,
            HeapObject::LStruct { .. } => TAG_STRUCT,
            HeapObject::LStructMut { .. } => TAG_STRUCT_MUT,
            HeapObject::Pair(_) => TAG_CONS,
            HeapObject::Closure { .. } => TAG_CLOSURE,
            HeapObject::LBytes { .. } => TAG_BYTES,
            HeapObject::LBytesMut { .. } => TAG_BYTES_MUT,
            HeapObject::LSet { .. } => TAG_SET,
            HeapObject::LSetMut { .. } => TAG_SET_MUT,
            HeapObject::LBox { .. } => TAG_LBOX,
            HeapObject::CaptureCell { .. } => TAG_CAPTURE_CELL,
            HeapObject::Fiber { .. } => TAG_FIBER,
            HeapObject::Syntax { .. } => TAG_SYNTAX,
            HeapObject::FFISignature(_, _) => TAG_FFI_SIG,
            HeapObject::FFIType(_) => TAG_FFI_TYPE,
            HeapObject::LibHandle(_) => TAG_LIB_HANDLE,
            HeapObject::ManagedPointer { .. } => TAG_MANAGED_PTR,
            HeapObject::External { .. } => TAG_EXTERNAL,
            HeapObject::Parameter { .. } => TAG_PARAMETER,
            HeapObject::ThreadHandle { .. } => TAG_THREAD,
            HeapObject::ClosureTemplate(_) => TAG_CLOSURE_TEMPLATE,
            // Float: in the new representation ALL floats are immediate (TAG_FLOAT,
            // payload = f64::to_bits()). HeapObject::Float must never be allocated.
            HeapObject::Float(_) => {
                panic!("HeapObject::Float must not be allocated — floats are now immediate")
            }
        }
    }

    /// Get a human-readable type name.
    pub fn type_name(&self) -> &'static str {
        match self {
            HeapObject::LString { .. } => "string",
            HeapObject::Pair(_) => "list",
            HeapObject::LArrayMut { .. } => "@array",
            HeapObject::LStructMut { .. } => "@struct",
            HeapObject::LStruct { .. } => "struct",
            HeapObject::Closure { .. } => "closure",
            HeapObject::LArray { .. } => "array",
            HeapObject::LStringMut { .. } => "@string",
            HeapObject::LBytes { .. } => "bytes",
            HeapObject::LBytesMut { .. } => "@bytes",
            HeapObject::LBox { .. } => "box",
            HeapObject::CaptureCell { .. } => "capture-cell",
            HeapObject::Float(_) => "float",
            HeapObject::LibHandle(_) => "library-handle",
            HeapObject::ThreadHandle { .. } => "thread-handle",
            HeapObject::Fiber { .. } => "fiber",
            HeapObject::Syntax { .. } => "syntax",
            HeapObject::FFISignature(_, _) => "ffi-signature",
            HeapObject::FFIType(_) => "ffi-type",
            HeapObject::ManagedPointer { .. } => "ptr",
            HeapObject::External { obj, .. } => obj.type_name,
            HeapObject::Parameter { .. } => "parameter",
            HeapObject::LSet { .. } => "set",
            HeapObject::LSetMut { .. } => "@set",
            HeapObject::ClosureTemplate(_) => "closure-template",
        }
    }
}

impl std::fmt::Debug for HeapObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HeapObject::LString { s, .. } => {
                write!(f, "\"{}\"", String::from_utf8_lossy(s.as_slice()))
            }
            HeapObject::Pair(c) => write!(f, "({:?} . {:?})", c.first, c.rest),
            HeapObject::LArrayMut { data, .. } => {
                if let Ok(borrowed) = data.try_borrow() {
                    write!(f, "{:?}", *borrowed)
                } else {
                    write!(f, "[<borrowed>]")
                }
            }
            HeapObject::LStructMut { .. } => write!(f, "<@struct>"),
            HeapObject::LStruct { .. } => write!(f, "<struct>"),
            HeapObject::Closure { .. } => write!(f, "<closure>"),
            HeapObject::LArray { elements, .. } => {
                write!(f, "[")?;
                for (i, v) in elements.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{:?}", v)?;
                }
                write!(f, "]")
            }
            HeapObject::LStringMut { data, .. } => {
                if let Ok(borrowed) = data.try_borrow() {
                    write!(f, "@\"{}\"", String::from_utf8_lossy(&borrowed))
                } else {
                    write!(f, "@\"<borrowed>\"")
                }
            }
            HeapObject::LBytes { data, .. } => {
                write!(f, "#bytes[")?;
                for (i, byte) in data.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{:02x}", byte)?;
                }
                write!(f, "]")
            }
            HeapObject::LBytesMut { data, .. } => {
                if let Ok(borrowed) = data.try_borrow() {
                    write!(f, "#@bytes[")?;
                    for (i, byte) in borrowed.iter().enumerate() {
                        if i > 0 {
                            write!(f, " ")?;
                        }
                        write!(f, "{:02x}", byte)?;
                    }
                    write!(f, "]")
                } else {
                    write!(f, "#@bytes[<borrowed>]")
                }
            }
            HeapObject::LBox { .. } => write!(f, "<box>"),
            HeapObject::CaptureCell { .. } => write!(f, "<capture-cell>"),
            HeapObject::Float(n) => write!(f, "{}", n),
            HeapObject::LibHandle(id) => write!(f, "<lib-handle:{}>", id),
            HeapObject::ThreadHandle { .. } => write!(f, "<thread-handle>"),
            HeapObject::Fiber { handle, .. } => match handle.try_with(|fib| fib.status.as_str()) {
                Some(status) => write!(f, "<fiber:{}>", status),
                None => write!(f, "<fiber:taken>"),
            },
            HeapObject::Syntax { syntax, .. } => write!(f, "#<syntax:{}>", syntax),
            HeapObject::FFISignature(_, _) => write!(f, "<ffi-signature>"),
            HeapObject::FFIType(desc) => write!(f, "<ffi-type:{:?}>", desc),
            HeapObject::ManagedPointer { addr, .. } => match addr.get() {
                Some(a) => write!(f, "<managed-pointer 0x{:x}>", a),
                None => write!(f, "<freed-pointer>"),
            },
            HeapObject::External { obj, .. } => write!(f, "#<{}>", obj.type_name),
            HeapObject::Parameter { id, .. } => write!(f, "<parameter:{}>", id),
            HeapObject::LSet { data, .. } => write!(f, "LSet({:?})", data),
            HeapObject::LSetMut { data, .. } => write!(f, "LSetMut({:?})", data.borrow()),
            HeapObject::ClosureTemplate(_) => write!(f, "<closure-template>"),
        }
    }
}
