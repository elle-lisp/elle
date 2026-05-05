//! Type interner for bidirectional type inference.
//!
//! Types are interned to `TyId(u32)`, following the same pattern as
//! `Binding(u32)` / `Region(u32)`. A `TypeInterner` owns all type data
//! and deduplicates structurally identical types.

/// Interned type handle. Only valid for the interner that created it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TyId(pub u32);

/// Internal type representation — stored in the interner, referenced by TyId.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) enum TyKind {
    Bottom,
    Nil,
    Bool,
    Int,
    Float,
    /// Supertype of Int and Float
    Number,
    String,
    Keyword,
    Symbol,
    EmptyList,
    Bytes,
    Array,
    MutableArray,
    Struct,
    MutableStruct,
    Top,
}

/// Interner: deduplicates types, provides O(1) lookup by TyId.
/// Currently only uses pre-interned constants; the `types` vec is
/// reserved for future compound types (Pair, Closure, etc.).
#[allow(dead_code)]
pub struct TypeInterner {
    types: Vec<TyKind>,
}

// Pre-interned constants — these are always at fixed indices.
impl TypeInterner {
    pub const BOTTOM: TyId = TyId(0);
    pub const TOP: TyId = TyId(1);
    pub const NIL: TyId = TyId(2);
    pub const BOOL: TyId = TyId(3);
    pub const INT: TyId = TyId(4);
    pub const FLOAT: TyId = TyId(5);
    pub const NUMBER: TyId = TyId(6);
    pub const STRING: TyId = TyId(7);
    pub const KEYWORD: TyId = TyId(8);
    pub const SYMBOL: TyId = TyId(9);
    pub const EMPTY_LIST: TyId = TyId(10);
    pub const BYTES: TyId = TyId(11);
    pub const ARRAY: TyId = TyId(12);
    pub const MUTABLE_ARRAY: TyId = TyId(13);
    pub const STRUCT: TyId = TyId(14);
    pub const MUTABLE_STRUCT: TyId = TyId(15);
}

impl Default for TypeInterner {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeInterner {
    pub fn new() -> Self {
        let preinterned = vec![
            TyKind::Bottom,
            TyKind::Top,
            TyKind::Nil,
            TyKind::Bool,
            TyKind::Int,
            TyKind::Float,
            TyKind::Number,
            TyKind::String,
            TyKind::Keyword,
            TyKind::Symbol,
            TyKind::EmptyList,
            TyKind::Bytes,
            TyKind::Array,
            TyKind::MutableArray,
            TyKind::Struct,
            TyKind::MutableStruct,
        ];
        TypeInterner { types: preinterned }
    }

    /// Least upper bound (join) of two types.
    pub fn join(&self, a: TyId, b: TyId) -> TyId {
        if a == b {
            return a;
        }
        if a == Self::BOTTOM {
            return b;
        }
        if b == Self::BOTTOM {
            return a;
        }
        if a == Self::TOP || b == Self::TOP {
            return Self::TOP;
        }
        // Int ∨ Float = Number
        if (a == Self::INT && b == Self::FLOAT) || (a == Self::FLOAT && b == Self::INT) {
            return Self::NUMBER;
        }
        // Int ∨ Number = Number, Float ∨ Number = Number
        if (a == Self::INT || a == Self::FLOAT) && b == Self::NUMBER {
            return Self::NUMBER;
        }
        if a == Self::NUMBER && (b == Self::INT || b == Self::FLOAT) {
            return Self::NUMBER;
        }
        Self::TOP
    }

    /// Greatest lower bound (meet) of two types.
    pub fn meet(&self, a: TyId, b: TyId) -> TyId {
        if a == b {
            return a;
        }
        if a == Self::TOP {
            return b;
        }
        if b == Self::TOP {
            return a;
        }
        if a == Self::BOTTOM || b == Self::BOTTOM {
            return Self::BOTTOM;
        }
        // Number ∧ Int = Int, Number ∧ Float = Float
        if a == Self::NUMBER && b == Self::INT {
            return Self::INT;
        }
        if a == Self::INT && b == Self::NUMBER {
            return Self::INT;
        }
        if a == Self::NUMBER && b == Self::FLOAT {
            return Self::FLOAT;
        }
        if a == Self::FLOAT && b == Self::NUMBER {
            return Self::FLOAT;
        }
        Self::BOTTOM
    }

    /// Subtype check: a ⊑ b
    pub fn subtype(&self, a: TyId, b: TyId) -> bool {
        if a == b {
            return true;
        }
        if a == Self::BOTTOM || b == Self::TOP {
            return true;
        }
        // Int ⊑ Number, Float ⊑ Number
        if b == Self::NUMBER && (a == Self::INT || a == Self::FLOAT) {
            return true;
        }
        false
    }

    /// Is this type an immediate (no heap allocation)?
    pub fn is_immediate(&self, id: TyId) -> bool {
        matches!(
            id,
            ty if ty == Self::INT
                || ty == Self::FLOAT
                || ty == Self::BOOL
                || ty == Self::NIL
                || ty == Self::KEYWORD
                || ty == Self::SYMBOL
                || ty == Self::NUMBER
        )
    }

    /// Is this type stringifiable (can be converted to string without error)?
    pub fn is_stringifiable(&self, id: TyId) -> bool {
        self.subtype(id, Self::NUMBER)
            || id == Self::STRING
            || id == Self::BOOL
            || id == Self::NIL
            || id == Self::KEYWORD
    }

    /// Is this a struct type (Struct or MutableStruct)?
    pub fn is_struct(&self, id: TyId) -> bool {
        id == Self::STRUCT || id == Self::MUTABLE_STRUCT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_same_type() {
        let i = TypeInterner::new();
        assert_eq!(
            i.join(TypeInterner::INT, TypeInterner::INT),
            TypeInterner::INT
        );
    }

    #[test]
    fn join_int_float_is_number() {
        let i = TypeInterner::new();
        assert_eq!(
            i.join(TypeInterner::INT, TypeInterner::FLOAT),
            TypeInterner::NUMBER
        );
    }

    #[test]
    fn join_int_string_is_top() {
        let i = TypeInterner::new();
        assert_eq!(
            i.join(TypeInterner::INT, TypeInterner::STRING),
            TypeInterner::TOP
        );
    }

    #[test]
    fn join_bottom_t() {
        let i = TypeInterner::new();
        assert_eq!(
            i.join(TypeInterner::BOTTOM, TypeInterner::STRING),
            TypeInterner::STRING
        );
    }

    #[test]
    fn subtype_int_number() {
        let i = TypeInterner::new();
        assert!(i.subtype(TypeInterner::INT, TypeInterner::NUMBER));
    }

    #[test]
    fn subtype_number_not_int() {
        let i = TypeInterner::new();
        assert!(!i.subtype(TypeInterner::NUMBER, TypeInterner::INT));
    }

    #[test]
    fn meet_number_int() {
        let i = TypeInterner::new();
        assert_eq!(
            i.meet(TypeInterner::NUMBER, TypeInterner::INT),
            TypeInterner::INT
        );
    }

    #[test]
    fn meet_int_string_is_bottom() {
        let i = TypeInterner::new();
        assert_eq!(
            i.meet(TypeInterner::INT, TypeInterner::STRING),
            TypeInterner::BOTTOM
        );
    }

    #[test]
    fn is_immediate_int() {
        let i = TypeInterner::new();
        assert!(i.is_immediate(TypeInterner::INT));
        assert!(i.is_immediate(TypeInterner::FLOAT));
        assert!(i.is_immediate(TypeInterner::BOOL));
        assert!(!i.is_immediate(TypeInterner::STRING));
        assert!(!i.is_immediate(TypeInterner::TOP));
    }

    #[test]
    fn join_array_mutable_array_is_top() {
        let i = TypeInterner::new();
        assert_eq!(
            i.join(TypeInterner::ARRAY, TypeInterner::MUTABLE_ARRAY),
            TypeInterner::TOP
        );
    }

    #[test]
    fn subtype_mutable_array_top() {
        let i = TypeInterner::new();
        assert!(i.subtype(TypeInterner::MUTABLE_ARRAY, TypeInterner::TOP));
    }

    #[test]
    fn is_immediate_array_false() {
        let i = TypeInterner::new();
        assert!(!i.is_immediate(TypeInterner::ARRAY));
        assert!(!i.is_immediate(TypeInterner::MUTABLE_ARRAY));
        assert!(!i.is_immediate(TypeInterner::STRUCT));
        assert!(!i.is_immediate(TypeInterner::MUTABLE_STRUCT));
    }

    #[test]
    fn is_stringifiable() {
        let i = TypeInterner::new();
        assert!(i.is_stringifiable(TypeInterner::INT));
        assert!(i.is_stringifiable(TypeInterner::STRING));
        assert!(i.is_stringifiable(TypeInterner::BOOL));
        assert!(i.is_stringifiable(TypeInterner::NIL));
        assert!(i.is_stringifiable(TypeInterner::KEYWORD));
        assert!(!i.is_stringifiable(TypeInterner::TOP));
        assert!(!i.is_stringifiable(TypeInterner::ARRAY));
    }

    #[test]
    fn is_struct() {
        let i = TypeInterner::new();
        assert!(i.is_struct(TypeInterner::STRUCT));
        assert!(i.is_struct(TypeInterner::MUTABLE_STRUCT));
        assert!(!i.is_struct(TypeInterner::ARRAY));
        assert!(!i.is_struct(TypeInterner::TOP));
    }
}
