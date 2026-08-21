//! Value → C marshalling: MarshalledArg and write_value_to_buffer.
//!
//! Converts Elle Values into C-typed storage for FFI arguments.
//!
//! The module root holds the storage types (`MarshalledArg`/`ArgStorage`) and
//! the `as_arg` accessor; the two large conversion paths live in submodules:
//! - `new`: `MarshalledArg::new` (Value → owned `ArgStorage`) + struct/array helpers.
//! - `buffer`: `write_value_to_buffer` (Value → caller-provided raw buffer).

use crate::ffi::marshal::AlignedBuffer;
use std::ffi::{c_void, CString};

mod buffer;
mod new;

pub use buffer::write_value_to_buffer;

/// Holds C-typed data for an FFI argument.
///
/// Must live as long as the `libffi::middle::Arg` references it.
/// Created from an Elle `Value` and a `TypeDesc`, then passed to
/// `ffi_call` via `as_arg()`.
pub struct MarshalledArg {
    // `pub(crate)` (not private) so the sibling `new`/`buffer` submodules can
    // construct `MarshalledArg` values directly.
    pub(crate) storage: ArgStorage,
}

// `pub(crate)` (not private) so sibling submodules that build `MarshalledArg`
// can name the variants.
#[allow(dead_code)]
pub(crate) enum ArgStorage {
    I8(i8),
    U8(u8),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    F32(f32),
    F64(f64),
    Ptr(*const c_void),
    /// Owned CString for `:string` type. The `*const c_char` is the
    /// pointer that libffi reads through (it's a `char*` argument).
    /// The CString must outlive the Arg.
    Str(CString, *const std::ffi::c_char),
    /// Struct/array data in an aligned buffer. The `Vec<MarshalledArg>`
    /// keeps CStrings and nested buffers alive for the buffer's lifetime.
    Struct(AlignedBuffer, Vec<MarshalledArg>),
}

impl MarshalledArg {
    /// Get a libffi Arg referencing this storage.
    pub fn as_arg(&self) -> libffi::middle::Arg<'_> {
        match &self.storage {
            ArgStorage::I8(v) => libffi::middle::arg(v),
            ArgStorage::U8(v) => libffi::middle::arg(v),
            ArgStorage::I16(v) => libffi::middle::arg(v),
            ArgStorage::U16(v) => libffi::middle::arg(v),
            ArgStorage::I32(v) => libffi::middle::arg(v),
            ArgStorage::U32(v) => libffi::middle::arg(v),
            ArgStorage::I64(v) => libffi::middle::arg(v),
            ArgStorage::U64(v) => libffi::middle::arg(v),
            ArgStorage::F32(v) => libffi::middle::arg(v),
            ArgStorage::F64(v) => libffi::middle::arg(v),
            ArgStorage::Ptr(v) => libffi::middle::arg(v),
            ArgStorage::Str(_, ptr) => libffi::middle::arg(ptr),
            ArgStorage::Struct(buf, _) => {
                // Safety: buf.as_mut_ptr() points to valid, aligned struct data that
                // outlives this Arg (the AlignedBuffer lives in ArgStorage).
                // Arg::new stores the pointer as *mut c_void; libffi reads the
                // struct data starting at that address.
                unsafe { libffi::middle::arg(&*buf.as_mut_ptr()) }
            }
        }
    }
}
