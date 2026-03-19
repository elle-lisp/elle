//! JIT value type with guaranteed C ABI layout.
//!
//! `JitValue` replaces the `(u64, u64)` tuple return type used by all
//! `extern "C"` JIT helpers. With `#[repr(C)]`, the two-field struct is
//! returned in rax:rdx on SystemV x86-64, which matches Cranelift's
//! two-I64 return convention.

/// A JIT Value represented as (tag, payload) with guaranteed C ABI layout.
/// Used as the return type for all JIT runtime helpers.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JitValue {
    pub tag: u64,
    pub payload: u64,
}

impl JitValue {
    #[inline]
    pub fn from_value(v: crate::value::Value) -> Self {
        JitValue {
            tag: v.tag,
            payload: v.payload,
        }
    }

    #[inline]
    pub fn to_value(self) -> crate::value::Value {
        crate::value::Value {
            tag: self.tag,
            payload: self.payload,
        }
    }

    #[inline]
    pub fn nil() -> Self {
        JitValue {
            tag: crate::value::repr::TAG_NIL,
            payload: 0,
        }
    }

    #[inline]
    pub fn bool_val(b: bool) -> Self {
        if b {
            JitValue {
                tag: crate::value::repr::TAG_TRUE,
                payload: 0,
            }
        } else {
            JitValue {
                tag: crate::value::repr::TAG_FALSE,
                payload: 0,
            }
        }
    }

    #[inline]
    pub fn empty_list() -> Self {
        JitValue {
            tag: crate::value::repr::TAG_EMPTY_LIST,
            payload: 0,
        }
    }
}

/// Sentinel returned when a JIT function performs a tail call.
pub const TAIL_CALL_SENTINEL_JV: JitValue = JitValue {
    tag: 0xDEAD_BEEF_DEAD_BEEFu64,
    payload: 0xDEAD_BEEF_DEAD_BEEFu64,
};

/// Sentinel returned when a JIT function yields (side-exits).
pub const YIELD_SENTINEL_JV: JitValue = JitValue {
    tag: 0xDEAD_CAFE_DEAD_CAFEu64,
    payload: 0xDEAD_CAFE_DEAD_CAFEu64,
};
