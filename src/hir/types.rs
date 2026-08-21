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
    MutableBytes,
    MutableString,
    Array,
    MutableArray,
    Struct,
    MutableStruct,
    Set,
    MutableSet,
    /// A cons cell. Flat (incomparable) like the container types; fed by the
    /// `%pair` constructor's return type and the `pair?` guards, consumed by
    /// the `%first`/`%rest` operand contract.
    Pair,
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
    pub const MUTABLE_STRING: TyId = TyId(16);
    pub const MUTABLE_BYTES: TyId = TyId(17);
    pub const SET: TyId = TyId(18);
    pub const MUTABLE_SET: TyId = TyId(19);
    pub const PAIR: TyId = TyId(20);
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
            // Appended at fixed indices 16/17 to match the TyId constants; the
            // mutability twins of String/Bytes are flat (incomparable) types, so
            // join/meet/subtype need no new cases.
            TyKind::MutableString,
            TyKind::MutableBytes,
            // Set/MutableSet at fixed indices 18/19. Flat incomparable types like the
            // String/Bytes twins above, so join/meet/subtype need no new cases.
            TyKind::Set,
            TyKind::MutableSet,
            // Pair at fixed index 20. Flat like the containers.
            TyKind::Pair,
        ];
        TypeInterner { types: preinterned }
    }

    /// Human-readable name for compile-error messages (the intrinsic operand
    /// proofs report what was inferred when they reject a site).
    pub fn describe(id: TyId) -> &'static str {
        match id {
            Self::BOTTOM => "unreachable",
            Self::TOP => "unknown",
            Self::NIL => "nil",
            Self::BOOL => "bool",
            Self::INT => "int",
            Self::FLOAT => "float",
            Self::NUMBER => "number",
            Self::STRING => "string",
            Self::KEYWORD => "keyword",
            Self::SYMBOL => "symbol",
            Self::EMPTY_LIST => "empty list",
            Self::BYTES => "bytes",
            Self::MUTABLE_BYTES => "@bytes",
            Self::MUTABLE_STRING => "@string",
            Self::ARRAY => "array",
            Self::MUTABLE_ARRAY => "@array",
            Self::STRUCT => "struct",
            Self::MUTABLE_STRUCT => "@struct",
            Self::SET => "set",
            Self::MUTABLE_SET => "@set",
            Self::PAIR => "pair",
            _ => "unknown",
        }
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
mod tests;
