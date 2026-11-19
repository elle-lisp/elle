// audited: 2026-11-18
//! The kind of a value: its place in the sequence the one order of all values ranks kinds by.
//!
//! docs/types.md

use crate::value::repr::{
    Value, TAG_ARRAY, TAG_ARRAY_MUT, TAG_BYTES, TAG_BYTES_MUT, TAG_CAPTURE_CELL, TAG_CLOSURE,
    TAG_CLOSURE_TEMPLATE, TAG_CONS, TAG_CPOINTER, TAG_EMPTY_LIST, TAG_EXTERNAL, TAG_FALSE,
    TAG_FFI_SIG, TAG_FFI_TYPE, TAG_FIBER, TAG_FLOAT, TAG_INT, TAG_KEYWORD, TAG_LBOX,
    TAG_LIB_HANDLE, TAG_MANAGED_PTR, TAG_NATIVE_FN, TAG_NIL, TAG_PARAMETER, TAG_SET, TAG_SET_MUT,
    TAG_STRING, TAG_STRING_MUT, TAG_STRUCT, TAG_STRUCT_MUT, TAG_SYMBOL, TAG_SYNTAX, TAG_THREAD,
    TAG_TRUE,
};

/// A value's kind. The declaration order is the order of kinds, so the derived
/// `Ord` is the comparison of two values of two kinds. A kind and its mutable
/// twin are one variant, which is what keeps mutability out of the order and
/// out of a hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Kind {
    Nil,
    Bool,
    Number,
    Symbol,
    Keyword,
    Pointer,
    String,
    EmptyList,
    Pair,
    Array,
    Bytes,
    Struct,
    Closure,
    Box,
    NativeFn,
    LibHandle,
    Thread,
    Fiber,
    Syntax,
    FfiSignature,
    FfiType,
    ManagedPointer,
    External,
    Parameter,
    Set,
    /// A closure template is never a value a program holds. It has a place so
    /// that a stray comparison is still total.
    ClosureTemplate,
    /// The compiler's undefined sentinel, and any tag this table does not name.
    Undefined,
}

impl Kind {
    /// The kind of `v`, read from its tag alone.
    #[inline]
    pub(crate) fn of(v: &Value) -> Kind {
        match v.tag {
            TAG_NIL => Kind::Nil,
            TAG_TRUE | TAG_FALSE => Kind::Bool,
            TAG_INT | TAG_FLOAT => Kind::Number,
            TAG_SYMBOL => Kind::Symbol,
            TAG_KEYWORD => Kind::Keyword,
            TAG_CPOINTER => Kind::Pointer,
            TAG_STRING | TAG_STRING_MUT => Kind::String,
            TAG_EMPTY_LIST => Kind::EmptyList,
            TAG_CONS => Kind::Pair,
            TAG_ARRAY | TAG_ARRAY_MUT => Kind::Array,
            TAG_BYTES | TAG_BYTES_MUT => Kind::Bytes,
            TAG_STRUCT | TAG_STRUCT_MUT => Kind::Struct,
            TAG_CLOSURE => Kind::Closure,
            TAG_LBOX | TAG_CAPTURE_CELL => Kind::Box,
            TAG_NATIVE_FN => Kind::NativeFn,
            TAG_LIB_HANDLE => Kind::LibHandle,
            TAG_THREAD => Kind::Thread,
            TAG_FIBER => Kind::Fiber,
            TAG_SYNTAX => Kind::Syntax,
            TAG_FFI_SIG => Kind::FfiSignature,
            TAG_FFI_TYPE => Kind::FfiType,
            TAG_MANAGED_PTR => Kind::ManagedPointer,
            TAG_EXTERNAL => Kind::External,
            TAG_PARAMETER => Kind::Parameter,
            TAG_SET | TAG_SET_MUT => Kind::Set,
            TAG_CLOSURE_TEMPLATE => Kind::ClosureTemplate,
            _ => Kind::Undefined,
        }
    }
}

/// Whether `v` is a container a program can store a value into after it is
/// built. A cycle of values runs through one of these on each side of a
/// comparison, so these are the values a traversal guards.
#[inline]
pub(in crate::value::repr) fn stores_values(v: &Value) -> bool {
    matches!(
        v.tag,
        TAG_ARRAY_MUT | TAG_STRUCT_MUT | TAG_SET_MUT | TAG_LBOX | TAG_CAPTURE_CELL
    )
}
