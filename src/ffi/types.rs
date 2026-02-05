//! C type system definition and layout calculation.
//!
//! This module defines the C types that Elle can work with via FFI,
//! including size and alignment calculations for the current platform.

use std::fmt;

/// Unique identifier for a C struct type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StructId(pub u32);

impl StructId {
    pub fn new(id: u32) -> Self {
        StructId(id)
    }
}

/// Unique identifier for a C enum type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnumId(pub u32);

impl EnumId {
    pub fn new(id: u32) -> Self {
        EnumId(id)
    }
}

/// Unique identifier for a C union type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnionId(pub u32);

impl UnionId {
    pub fn new(id: u32) -> Self {
        UnionId(id)
    }
}

/// A C type that can be marshaled to/from Elle values.
///
/// # Supported Types
/// - Void
/// - Bool
/// - Char, Short, Int, Long, LongLong
/// - Float, Double
/// - Pointer types (including opaque pointers)
/// - Struct types
/// - Enum types
/// - Array types
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CType {
    Void,
    Bool,
    Char,
    SChar,
    UChar,
    Short,
    UShort,
    Int,
    UInt,
    Long,
    ULong,
    LongLong,
    ULongLong,
    Float,
    Double,
    /// Opaque pointer - used for function pointers and C object handles
    Pointer(Box<CType>),
    /// C struct type identified by StructId
    Struct(StructId),
    /// C enum type identified by EnumId
    Enum(EnumId),
    /// C union type identified by UnionId
    Union(UnionId),
    /// C array type
    Array(Box<CType>, usize),
}

impl CType {
    /// Get the size of this type in bytes (x86-64 Linux ABI).
    pub fn size(&self) -> usize {
        match self {
            CType::Void => 0,
            CType::Bool => 1,
            CType::Char | CType::SChar | CType::UChar => 1,
            CType::Short | CType::UShort => 2,
            CType::Int | CType::UInt => 4,
            CType::Long | CType::ULong => 8,
            CType::LongLong | CType::ULongLong => 8,
            CType::Float => 4,
            CType::Double => 8,
            CType::Pointer(_) => 8, // x86-64: pointers are 8 bytes
            CType::Struct(_) => panic!("Struct size must be queried from layout"),
            CType::Enum(_) => 4, // enums are typically int-sized
            CType::Union(_) => panic!("Union size must be queried from layout"),
            CType::Array(elem_type, count) => elem_type.size() * count,
        }
    }

    /// Get the alignment of this type in bytes (x86-64 Linux ABI).
    pub fn alignment(&self) -> usize {
        match self {
            CType::Void => 0,
            CType::Pointer(_) => 8,
            CType::Struct(_) => panic!("Struct alignment must be queried from layout"),
            CType::Union(_) => panic!("Union alignment must be queried from layout"),
            CType::Array(elem_type, _) => elem_type.alignment(),
            _ => self.size(),
        }
    }

    /// Check if this is an integer type.
    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            CType::Bool
                | CType::Char
                | CType::SChar
                | CType::UChar
                | CType::Short
                | CType::UShort
                | CType::Int
                | CType::UInt
                | CType::Long
                | CType::ULong
                | CType::LongLong
                | CType::ULongLong
        )
    }

    /// Check if this is a floating-point type.
    pub fn is_float(&self) -> bool {
        matches!(self, CType::Float | CType::Double)
    }

    /// Check if this is a pointer type.
    pub fn is_pointer(&self) -> bool {
        matches!(self, CType::Pointer(_))
    }

    /// Check if this is a struct type.
    pub fn is_struct(&self) -> bool {
        matches!(self, CType::Struct(_))
    }

    /// Check if this is an array type.
    pub fn is_array(&self) -> bool {
        matches!(self, CType::Array(_, _))
    }

    /// Check if this is an enum type.
    pub fn is_enum(&self) -> bool {
        matches!(self, CType::Enum(_))
    }

    /// Check if this is a union type.
    pub fn is_union(&self) -> bool {
        matches!(self, CType::Union(_))
    }
}

impl fmt::Display for CType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CType::Void => write!(f, "void"),
            CType::Bool => write!(f, "bool"),
            CType::Char => write!(f, "char"),
            CType::SChar => write!(f, "signed char"),
            CType::UChar => write!(f, "unsigned char"),
            CType::Short => write!(f, "short"),
            CType::UShort => write!(f, "unsigned short"),
            CType::Int => write!(f, "int"),
            CType::UInt => write!(f, "unsigned int"),
            CType::Long => write!(f, "long"),
            CType::ULong => write!(f, "unsigned long"),
            CType::LongLong => write!(f, "long long"),
            CType::ULongLong => write!(f, "unsigned long long"),
            CType::Float => write!(f, "float"),
            CType::Double => write!(f, "double"),
            CType::Pointer(inner) => write!(f, "{}*", inner),
            CType::Struct(id) => write!(f, "struct_{:?}", id),
            CType::Enum(id) => write!(f, "enum_{:?}", id),
            CType::Union(id) => write!(f, "union_{:?}", id),
            CType::Array(elem, count) => write!(f, "{}[{}]", elem, count),
        }
    }
}

/// A single field within a C struct.
#[derive(Debug, Clone)]
pub struct StructField {
    pub name: String,
    pub ctype: CType,
    pub offset: usize,
}

/// Layout information for a C struct type.
#[derive(Debug, Clone)]
pub struct StructLayout {
    pub id: StructId,
    pub name: String,
    pub fields: Vec<StructField>,
    pub size: usize,
    pub align: usize,
}

impl StructLayout {
    /// Create a new struct layout.
    pub fn new(
        id: StructId,
        name: String,
        fields: Vec<StructField>,
        size: usize,
        align: usize,
    ) -> Self {
        StructLayout {
            id,
            name,
            fields,
            size,
            align,
        }
    }

    /// Get the offset of a field by name.
    pub fn field_offset(&self, name: &str) -> Option<usize> {
        self.fields
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.offset)
    }

    /// Get a field by name.
    pub fn get_field(&self, name: &str) -> Option<&StructField> {
        self.fields.iter().find(|f| f.name == name)
    }
}

/// An enum variant in a C enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumVariant {
    pub name: String,
    pub value: i64,
}

/// Layout information for a C enum type.
#[derive(Debug, Clone)]
pub struct EnumLayout {
    pub id: EnumId,
    pub name: String,
    pub variants: Vec<EnumVariant>,
    pub base_type: CType,
}

impl EnumLayout {
    /// Create a new enum layout.
    pub fn new(id: EnumId, name: String, variants: Vec<EnumVariant>, base_type: CType) -> Self {
        EnumLayout {
            id,
            name,
            variants,
            base_type,
        }
    }

    /// Get the value of a variant by name.
    pub fn variant_value(&self, name: &str) -> Option<i64> {
        self.variants
            .iter()
            .find(|v| v.name == name)
            .map(|v| v.value)
    }

    /// Get a variant by name.
    pub fn get_variant(&self, name: &str) -> Option<&EnumVariant> {
        self.variants.iter().find(|v| v.name == name)
    }
}

/// A single field within a C union.
///
/// Unlike struct fields, all union fields start at offset 0 and overlap in memory.
#[derive(Debug, Clone)]
pub struct UnionField {
    pub name: String,
    pub ctype: CType,
}

/// Layout information for a C union type.
///
/// A union stores all fields at the same memory location (offset 0).
/// The size of the union equals the size of its largest field.
/// The alignment of the union equals the alignment of its largest field.
#[derive(Debug, Clone)]
pub struct UnionLayout {
    pub id: UnionId,
    pub name: String,
    pub fields: Vec<UnionField>,
    pub size: usize,
    pub align: usize,
}

impl UnionLayout {
    /// Create a new union layout.
    pub fn new(
        id: UnionId,
        name: String,
        fields: Vec<UnionField>,
        size: usize,
        align: usize,
    ) -> Self {
        UnionLayout {
            id,
            name,
            fields,
            size,
            align,
        }
    }

    /// Get a field by name.
    pub fn get_field(&self, name: &str) -> Option<&UnionField> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// Check if a field exists by name.
    pub fn has_field(&self, name: &str) -> bool {
        self.fields.iter().any(|f| f.name == name)
    }
}

/// Function signature for a C function.
#[derive(Debug, Clone)]
pub struct FunctionSignature {
    /// Function name (as it appears in the library)
    pub name: String,
    /// Argument types
    pub args: Vec<CType>,
    /// Return type
    pub return_type: CType,
    /// Whether this is a variadic function (not yet supported)
    pub variadic: bool,
}

impl FunctionSignature {
    /// Create a new function signature.
    pub fn new(name: String, args: Vec<CType>, return_type: CType) -> Self {
        FunctionSignature {
            name,
            args,
            return_type,
            variadic: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_sizes() {
        assert_eq!(CType::Void.size(), 0);
        assert_eq!(CType::Bool.size(), 1);
        assert_eq!(CType::Char.size(), 1);
        assert_eq!(CType::Short.size(), 2);
        assert_eq!(CType::Int.size(), 4);
        assert_eq!(CType::Long.size(), 8);
        assert_eq!(CType::LongLong.size(), 8);
        assert_eq!(CType::Float.size(), 4);
        assert_eq!(CType::Double.size(), 8);
        assert_eq!(CType::Pointer(Box::new(CType::Int)).size(), 8);
    }

    #[test]
    fn test_type_alignment() {
        assert_eq!(CType::Bool.alignment(), 1);
        assert_eq!(CType::Short.alignment(), 2);
        assert_eq!(CType::Int.alignment(), 4);
        assert_eq!(CType::Long.alignment(), 8);
        assert_eq!(CType::Double.alignment(), 8);
        assert_eq!(CType::Pointer(Box::new(CType::Int)).alignment(), 8);
    }

    #[test]
    fn test_type_classification() {
        assert!(CType::Int.is_integer());
        assert!(!CType::Float.is_integer());
        assert!(!CType::Double.is_integer());
        assert!(CType::Float.is_float());
        assert!(CType::Double.is_float());
        assert!(!CType::Int.is_float());
        assert!(CType::Pointer(Box::new(CType::Int)).is_pointer());
        assert!(CType::Array(Box::new(CType::Int), 10).is_array());
    }

    #[test]
    fn test_array_size() {
        let array_type = CType::Array(Box::new(CType::Int), 10);
        assert_eq!(array_type.size(), 40); // 4 bytes * 10
    }

    #[test]
    fn test_struct_layout() {
        let fields = vec![
            StructField {
                name: "x".to_string(),
                ctype: CType::Int,
                offset: 0,
            },
            StructField {
                name: "y".to_string(),
                ctype: CType::Int,
                offset: 4,
            },
        ];
        let layout = StructLayout::new(StructId::new(1), "Point".to_string(), fields, 8, 4);

        assert_eq!(layout.size, 8);
        assert_eq!(layout.field_offset("x"), Some(0));
        assert_eq!(layout.field_offset("y"), Some(4));
        assert_eq!(layout.field_offset("z"), None);
    }
}
