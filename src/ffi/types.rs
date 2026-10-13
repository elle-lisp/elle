// audited: 2026-10-13
//! FFI type descriptors: the C type each keyword names, its size and alignment, and the storage of a C boolean.
//!
//! docs/ffi.md

use crate::value::Value;

/// The storage of `TypeDesc::Bool`, the C `bool`. Read it as a byte, never as
/// a Rust `bool`: a byte other than 0 or 1 makes a `bool` read undefined.
pub type CBool = u8;

/// The storage of `TypeDesc::Bool32`, a boolean held in a 32-bit integer.
pub type CBool32 = i32;

/// The C truth value of `value`: 1 when it is truthy, 0 when it is falsy.
pub fn to_c_bool<T: From<u8>>(value: &Value) -> T {
    T::from(u8::from(value.is_truthy()))
}

/// The Elle boolean of the C truth value `c`: true when it is nonzero.
pub fn from_c_bool<T: Into<i64>>(c: T) -> Value {
    Value::bool(c.into() != 0)
}

/// Describes a C type for marshalling.
#[derive(Debug, Clone, PartialEq, Hash)]
pub enum TypeDesc {
    Void,
    /// C `bool`, stored as a `CBool`
    Bool,
    /// A boolean held in a 32-bit integer, stored as a `CBool32`
    Bool32,
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    Float,
    Double,
    /// Platform-dependent `int`
    Int,
    /// Platform-dependent `unsigned int`
    UInt,
    /// Platform-dependent `long`
    Long,
    /// Platform-dependent `unsigned long`
    ULong,
    /// Platform-dependent `char` (signed on most platforms)
    Char,
    /// `unsigned char`
    UChar,
    /// `short`
    Short,
    /// `unsigned short`
    UShort,
    /// `size_t`
    Size,
    /// `ptrdiff_t`
    SSize,
    /// `void *` — maps to `Value::pointer()` or `nil` for NULL
    Ptr,
    /// `const char *` — marshalled as Elle string (copied)
    Str,
    /// Struct with positional fields
    Struct(StructDesc),
    /// Fixed-size array: element type + count
    Array(Box<TypeDesc>, usize),
}

/// Positional struct descriptor.
///
/// Fields are unnamed and ordered. Created via `ffi/struct` at the Elle level.
#[derive(Debug, Clone, PartialEq, Hash)]
pub struct StructDesc {
    pub fields: Vec<TypeDesc>,
}

impl StructDesc {
    /// Compute the byte offset of each field within the struct layout.
    ///
    /// Returns `(offsets, total_size)` where `offsets[i]` is the byte offset
    /// of field `i`, and `total_size` includes tail padding.
    /// Returns `None` if any field has unknown size/alignment (e.g., contains Void).
    pub fn field_offsets(&self) -> Option<(Vec<usize>, usize)> {
        let mut offsets = Vec::with_capacity(self.fields.len());
        let mut offset = 0usize;
        for field in &self.fields {
            let field_align = field.align()?;
            offset = (offset + field_align - 1) & !(field_align - 1);
            offsets.push(offset);
            offset += field.size()?;
        }
        // Tail padding: align total size to struct alignment
        let struct_align = self
            .fields
            .iter()
            .filter_map(|f| f.align())
            .max()
            .unwrap_or(1);
        offset = (offset + struct_align - 1) & !(struct_align - 1);
        Some((offsets, offset))
    }
}

impl TypeDesc {
    /// Parse a type descriptor from an Elle keyword name.
    ///
    /// Returns `None` for unrecognized keywords.
    pub fn from_keyword(name: &str) -> Option<Self> {
        match name {
            "void" => Some(TypeDesc::Void),
            "bool" => Some(TypeDesc::Bool),
            "bool32" => Some(TypeDesc::Bool32),
            "i8" => Some(TypeDesc::I8),
            "u8" => Some(TypeDesc::U8),
            "i16" => Some(TypeDesc::I16),
            "u16" => Some(TypeDesc::U16),
            "i32" => Some(TypeDesc::I32),
            "u32" => Some(TypeDesc::U32),
            "i64" => Some(TypeDesc::I64),
            "u64" => Some(TypeDesc::U64),
            "float" => Some(TypeDesc::Float),
            "double" => Some(TypeDesc::Double),
            "int" => Some(TypeDesc::Int),
            "uint" => Some(TypeDesc::UInt),
            "long" => Some(TypeDesc::Long),
            "ulong" => Some(TypeDesc::ULong),
            "char" => Some(TypeDesc::Char),
            "uchar" => Some(TypeDesc::UChar),
            "short" => Some(TypeDesc::Short),
            "ushort" => Some(TypeDesc::UShort),
            "size" => Some(TypeDesc::Size),
            "ssize" => Some(TypeDesc::SSize),
            "ptr" => Some(TypeDesc::Ptr),
            "string" => Some(TypeDesc::Str),
            _ => None,
        }
    }

    /// Size of this type in bytes on the current platform.
    ///
    /// Returns `None` for `Void`.
    pub fn size(&self) -> Option<usize> {
        match self {
            TypeDesc::Void => None,
            TypeDesc::Bool => Some(std::mem::size_of::<CBool>()),
            TypeDesc::Bool32 => Some(std::mem::size_of::<CBool32>()),
            TypeDesc::I8 | TypeDesc::U8 => Some(1),
            TypeDesc::I16 | TypeDesc::U16 => Some(2),
            TypeDesc::I32 | TypeDesc::U32 => Some(4),
            TypeDesc::I64 | TypeDesc::U64 => Some(8),
            TypeDesc::Float => Some(4),
            TypeDesc::Double => Some(8),
            TypeDesc::Int | TypeDesc::UInt => Some(std::mem::size_of::<std::ffi::c_int>()),
            TypeDesc::Long | TypeDesc::ULong => Some(std::mem::size_of::<std::ffi::c_long>()),
            TypeDesc::Char | TypeDesc::UChar => Some(1),
            TypeDesc::Short | TypeDesc::UShort => Some(std::mem::size_of::<std::ffi::c_short>()),
            TypeDesc::Size => Some(std::mem::size_of::<usize>()),
            TypeDesc::SSize => Some(std::mem::size_of::<isize>()),
            TypeDesc::Ptr | TypeDesc::Str => Some(std::mem::size_of::<*const ()>()),
            TypeDesc::Struct(desc) => desc.field_offsets().map(|(_, total_size)| total_size),
            TypeDesc::Array(elem, count) => elem.size().map(|s| s * count),
        }
    }

    /// Alignment of this type in bytes on the current platform.
    ///
    /// Returns `None` for `Void`.
    pub fn align(&self) -> Option<usize> {
        match self {
            TypeDesc::Void => None,
            TypeDesc::Bool => Some(std::mem::align_of::<CBool>()),
            TypeDesc::Bool32 => Some(std::mem::align_of::<CBool32>()),
            TypeDesc::I8 | TypeDesc::U8 => Some(1),
            TypeDesc::I16 | TypeDesc::U16 => Some(2),
            TypeDesc::I32 | TypeDesc::U32 => Some(4),
            TypeDesc::I64 | TypeDesc::U64 => Some(8),
            TypeDesc::Float => Some(4),
            TypeDesc::Double => Some(8),
            TypeDesc::Int | TypeDesc::UInt => Some(std::mem::align_of::<std::ffi::c_int>()),
            TypeDesc::Long | TypeDesc::ULong => Some(std::mem::align_of::<std::ffi::c_long>()),
            TypeDesc::Char | TypeDesc::UChar => Some(1),
            TypeDesc::Short | TypeDesc::UShort => Some(std::mem::align_of::<std::ffi::c_short>()),
            TypeDesc::Size => Some(std::mem::align_of::<usize>()),
            TypeDesc::SSize => Some(std::mem::align_of::<isize>()),
            TypeDesc::Ptr | TypeDesc::Str => Some(std::mem::align_of::<*const ()>()),
            TypeDesc::Struct(desc) => {
                // Alignment is the max alignment of any field
                desc.fields
                    .iter()
                    .filter_map(|f| f.align())
                    .max()
                    .or(Some(1))
            }
            TypeDesc::Array(elem, _) => elem.align(),
        }
    }

    /// Short display name for this type descriptor.
    pub fn short_name(&self) -> String {
        match self {
            TypeDesc::Void => "void".to_string(),
            TypeDesc::Bool => "bool".to_string(),
            TypeDesc::Bool32 => "bool32".to_string(),
            TypeDesc::I8 => "i8".to_string(),
            TypeDesc::U8 => "u8".to_string(),
            TypeDesc::I16 => "i16".to_string(),
            TypeDesc::U16 => "u16".to_string(),
            TypeDesc::I32 => "i32".to_string(),
            TypeDesc::U32 => "u32".to_string(),
            TypeDesc::I64 => "i64".to_string(),
            TypeDesc::U64 => "u64".to_string(),
            TypeDesc::Float => "float".to_string(),
            TypeDesc::Double => "double".to_string(),
            TypeDesc::Int => "int".to_string(),
            TypeDesc::UInt => "uint".to_string(),
            TypeDesc::Long => "long".to_string(),
            TypeDesc::ULong => "ulong".to_string(),
            TypeDesc::Char => "char".to_string(),
            TypeDesc::UChar => "uchar".to_string(),
            TypeDesc::Short => "short".to_string(),
            TypeDesc::UShort => "ushort".to_string(),
            TypeDesc::Size => "size".to_string(),
            TypeDesc::SSize => "ssize".to_string(),
            TypeDesc::Ptr => "ptr".to_string(),
            TypeDesc::Str => "string".to_string(),
            TypeDesc::Struct(sd) => format!("struct({})", sd.fields.len()),
            TypeDesc::Array(elem, count) => format!("array({}, {})", elem.short_name(), count),
        }
    }
}

/// Reified function signature for FFI calls.
///
/// Created by `ffi/signature`. Contains calling convention, return type,
/// and argument types. The signature value caches the libffi CIF prepared
/// from it, so each value prepares one CIF.
#[derive(Debug, Clone, PartialEq, Hash)]
pub struct Signature {
    /// Calling convention (currently only `:default`)
    pub convention: CallingConvention,
    /// Return type
    pub ret: TypeDesc,
    /// Argument types
    pub args: Vec<TypeDesc>,
    /// For variadic functions: number of fixed arguments.
    /// `None` means non-variadic (all args are fixed).
    pub fixed_args: Option<usize>,
}

/// Calling convention for FFI functions.
#[derive(Debug, Clone, Copy, PartialEq, Hash)]
pub enum CallingConvention {
    /// Platform default calling convention
    Default,
}

impl CallingConvention {
    /// Parse from keyword name.
    pub fn from_keyword(name: &str) -> Option<Self> {
        match name {
            "default" => Some(CallingConvention::Default),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
