//! The declarative contract table: one soundness row per intrinsic op.
//!
//! `op_contract` maps every `IntrinsicOp` onto a `Contract` row exhaustively —
//! a new op fails the build until it declares what its lowering trusts.

use super::*;

/// The type set a container op's first operand may inhabit.
pub(super) const ARRAY_FAMILY: &[TyId] = &[TypeInterner::ARRAY, TypeInterner::MUTABLE_ARRAY];
pub(super) const STRUCT_FAMILY: &[TyId] = &[TypeInterner::STRUCT, TypeInterner::MUTABLE_STRUCT];
const STRING_FAMILY: &[TyId] = &[TypeInterner::STRING, TypeInterner::MUTABLE_STRING];
const BYTES_FAMILY: &[TyId] = &[TypeInterner::BYTES, TypeInterner::MUTABLE_BYTES];
const SET_FAMILY: &[TyId] = &[TypeInterner::SET, TypeInterner::MUTABLE_SET];

/// What an op's lowering trusts its operands to satisfy. One row per shape;
/// `op_contract` maps every `IntrinsicOp` onto a row (exhaustively — a new op
/// fails the build until it declares its contract).
pub(super) enum Contract {
    /// Total on every value (equality, identity, truthiness, predicates,
    /// `%type-of`, `%pair`, and the pass-through `%freeze`/`%thaw`).
    Total,
    /// Every operand ⊑ Number (wrapping arithmetic, unary negate, conversions).
    Numbers,
    /// Every operand ⊑ Number AND the divisor (last operand) provably nonzero.
    DivFamily,
    /// Every operand ⊑ Int (bitwise and shifts).
    Ints,
    /// Both operands in one comparable family: Number/Number, string/string,
    /// keyword/keyword (the ordering opcodes compare exactly these).
    Ordered,
    /// First operand a pair (`%first`/`%rest` — their opcodes trust the cell).
    PairArg,
    /// First operand's type ∈ the listed set, described as `what`.
    Container {
        families: &'static [TyId],
        what: &'static str,
    },
    /// `%get`: container-dependent key legality (array/string index ⊑ Int;
    /// struct keys proven hashable — the surface `get` raises :type-error for
    /// an unhashable key, and the opcode's unreachable-by-proof path panics).
    Get,
}

/// The lengths (`%length`) domain: every container, plus lists (pair chains,
/// the empty list) and nil — exactly the cases its opcode handles.
const LENGTH_DOMAIN: &[TyId] = &[
    TypeInterner::ARRAY,
    TypeInterner::MUTABLE_ARRAY,
    TypeInterner::STRUCT,
    TypeInterner::MUTABLE_STRUCT,
    TypeInterner::SET,
    TypeInterner::MUTABLE_SET,
    TypeInterner::STRING,
    TypeInterner::MUTABLE_STRING,
    TypeInterner::BYTES,
    TypeInterner::MUTABLE_BYTES,
    TypeInterner::PAIR,
    TypeInterner::EMPTY_LIST,
    TypeInterner::NIL,
];

const HAS_DOMAIN: &[TyId] = &[
    TypeInterner::STRUCT,
    TypeInterner::MUTABLE_STRUCT,
    TypeInterner::SET,
    TypeInterner::MUTABLE_SET,
    TypeInterner::STRING,
    TypeInterner::MUTABLE_STRING,
];

const PUT_DOMAIN: &[TyId] = &[
    TypeInterner::STRUCT,
    TypeInterner::MUTABLE_STRUCT,
    TypeInterner::ARRAY,
    TypeInterner::MUTABLE_ARRAY,
];

const DEL_DOMAIN: &[TyId] = &[
    TypeInterner::STRUCT,
    TypeInterner::MUTABLE_STRUCT,
    TypeInterner::SET,
    TypeInterner::MUTABLE_SET,
];

const POP_DOMAIN: &[TyId] = &[TypeInterner::MUTABLE_ARRAY];

pub(super) fn op_contract(op: IntrinsicOp) -> Contract {
    use IntrinsicOp::*;
    match op {
        Add | Sub | Mul => Contract::Numbers,
        Div | Rem | Mod => Contract::DivFamily,
        Int | Float => Contract::Numbers,
        BitAnd | BitOr | BitXor | BitNot | Shl | Shr => Contract::Ints,
        Lt | Gt | Le | Ge => Contract::Ordered,
        Eq | Ne | Identical | Not | Pair | TypeOf => Contract::Total,
        IsNil | IsEmpty | IsBool | IsInt | IsFloat | IsString | IsKeyword | IsSymbol | IsPair
        | IsArray | IsStruct | IsSet | IsBytes | IsBox | IsClosure | IsFiber => Contract::Total,
        First | Rest => Contract::PairArg,
        Length => Contract::Container {
            families: LENGTH_DOMAIN,
            what: "container, list, or nil",
        },
        Get => Contract::Get,
        Has => Contract::Container {
            families: HAS_DOMAIN,
            what: "struct, set, or string",
        },
        // The monomorphic put/push variants pin the family; mutability is the
        // runtime dispatch's business (both mutabilities are family-legal, the
        // same gate the monomorphization obligation always held).
        Put => Contract::Container {
            families: PUT_DOMAIN,
            what: "struct or array",
        },
        PutStruct | PutStructMut => Contract::Container {
            families: STRUCT_FAMILY,
            what: "struct",
        },
        PutArray | PutArrayMut => Contract::Container {
            families: ARRAY_FAMILY,
            what: "array",
        },
        Push | PushArray | PushArrayMut => Contract::Container {
            families: ARRAY_FAMILY,
            what: "array",
        },
        // Set add pins the set family; the immutable-vs-mutable split is the
        // runtime dispatch's business, like the put/push monomorphic variants.
        AddSet | AddSetMut => Contract::Container {
            families: SET_FAMILY,
            what: "set",
        },
        Del => Contract::Container {
            families: DEL_DOMAIN,
            what: "struct or set",
        },
        // The monomorphic del variants pin the family; the immutable-vs-mutable
        // split is the runtime dispatch's business, like the put/push/add variants.
        DelStruct | DelStructMut => Contract::Container {
            families: STRUCT_FAMILY,
            what: "struct",
        },
        DelSet | DelSetMut => Contract::Container {
            families: SET_FAMILY,
            what: "set",
        },
        Pop => Contract::Container {
            families: POP_DOMAIN,
            what: "@array",
        },
        // The storing ops' compile gate owns the *container* (the operand the
        // region system and the opcode's dispatch trust); the pushed value's
        // legality is the funnel native's runtime validation, which signals
        // like any native (`prim_string_push` / `prim_bytes_push`).
        StringPush => Contract::Container {
            families: STRING_FAMILY,
            what: "string",
        },
        BytesPush => Contract::Container {
            families: BYTES_FAMILY,
            what: "bytes",
        },
        // Pass-throughs on already-right-mutability inputs, copies otherwise;
        // total on every value.
        Freeze | Thaw => Contract::Total,
    }
}
