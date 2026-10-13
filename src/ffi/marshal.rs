// audited: 2026-10-13
//! Marshalling helpers shared by both directions: the libffi type of a descriptor, and the aligned buffer.
//!
//! docs/ffi.md
//!
//! Value→C conversion is in `to_c.rs`, C→Value conversion is in `from_c.rs`,
//! and the scalar checks both use are in `scalar.rs`.

use crate::ffi::types::TypeDesc;
use libffi::middle::Type;

// One import path for both conversion directions.
pub(crate) use crate::ffi::from_c::read_value_from_buffer;
pub(crate) use crate::ffi::to_c::write_value_to_buffer;
pub use crate::ffi::to_c::MarshalledArg;

/// Convert a `TypeDesc` to the corresponding `libffi::middle::Type`.
pub(crate) fn to_libffi_type(desc: &TypeDesc) -> Type {
    match desc {
        TypeDesc::Void => Type::void(),
        // `CBool` and `CBool32`
        TypeDesc::Bool => Type::u8(),
        TypeDesc::Bool32 => Type::i32(),
        TypeDesc::I8 => Type::i8(),
        TypeDesc::U8 => Type::u8(),
        TypeDesc::I16 => Type::i16(),
        TypeDesc::U16 => Type::u16(),
        TypeDesc::I32 => Type::i32(),
        TypeDesc::U32 => Type::u32(),
        TypeDesc::I64 => Type::i64(),
        TypeDesc::U64 => Type::u64(),
        TypeDesc::Float => Type::f32(),
        TypeDesc::Double => Type::f64(),
        TypeDesc::Int => Type::c_int(),
        TypeDesc::UInt => Type::c_uint(),
        TypeDesc::Long => Type::c_long(),
        TypeDesc::ULong => Type::c_ulong(),
        TypeDesc::Char => Type::i8(),
        TypeDesc::UChar => Type::u8(),
        TypeDesc::Short => Type::c_short(),
        TypeDesc::UShort => Type::c_ushort(),
        TypeDesc::Size => Type::usize(),
        TypeDesc::SSize => Type::isize(),
        TypeDesc::Ptr | TypeDesc::Str => Type::pointer(),
        TypeDesc::Struct(desc) => {
            let fields: Vec<Type> = desc.fields.iter().map(to_libffi_type).collect();
            Type::structure(fields)
        }
        TypeDesc::Array(elem, count) => {
            let elem_type = to_libffi_type(elem);
            let fields: Vec<Type> = (0..*count).map(|_| elem_type.clone()).collect();
            Type::structure(fields)
        }
    }
}

// ── Aligned buffer ──────────────────────────────────────────────────

/// Heap-allocated buffer with guaranteed alignment for FFI struct data.
///
/// Used to hold C struct/array data that libffi reads from (arguments)
/// or writes into (return values). The buffer is zero-initialized.
pub(crate) struct AlignedBuffer {
    ptr: *mut u8,
    layout: std::alloc::Layout,
}

impl AlignedBuffer {
    /// Allocate a zero-initialized buffer of `size` bytes with `align` alignment.
    ///
    /// Panics if align is 0 or not a power of two, or if size overflows
    /// the layout constraints.
    pub(crate) fn new(size: usize, align: usize) -> Self {
        // std::alloc::alloc with size 0 is UB; use at least 1 byte.
        let effective_size = size.max(1);
        let layout = std::alloc::Layout::from_size_align(effective_size, align)
            .expect("invalid layout for AlignedBuffer");
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        AlignedBuffer { ptr, layout }
    }

    /// Raw pointer to the buffer data.
    pub(crate) fn as_mut_ptr(&self) -> *mut u8 {
        self.ptr
    }
}

impl Drop for AlignedBuffer {
    fn drop(&mut self) {
        unsafe {
            std::alloc::dealloc(self.ptr, self.layout);
        }
    }
}

#[cfg(test)]
mod tests;
