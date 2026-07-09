//! Opcode handlers for type introspection and collection intrinsics. Split by
//! concern into two submodules whose items are re-exported here so callers keep
//! reaching them as `crate::vm::types::handle_*`:
//!   - `predicate`: non-allocating `is-*`/`type-of`/`length`/`identical?`/`ne`/`bit-not`.
//!   - `intrinsic`: allocating/mutating `%get`/`%put`/`%del`/`*-push`/`%pop`/`%freeze`/`%thaw`.

mod intrinsic;
mod predicate;

// Re-exported for the `tests` submodule, whose `use super::*` relied on these
// being imported into this root before the split.
#[cfg(test)]
use crate::value::Value;
#[cfg(test)]
use crate::vm::core::VM;

pub(crate) use intrinsic::{
    handle_intr_bytes_push, handle_intr_del, handle_intr_freeze, handle_intr_get, handle_intr_has,
    handle_intr_pop, handle_intr_push, handle_intr_put, handle_intr_string_push, handle_intr_thaw,
    run_alloc_intrinsic,
};
pub(crate) use predicate::{
    handle_array_len, handle_bit_not_intr, handle_identical, handle_is_array, handle_is_array_mut,
    handle_is_bool, handle_is_box, handle_is_bytes, handle_is_closure, handle_is_empty_list,
    handle_is_fiber, handle_is_float, handle_is_int, handle_is_keyword, handle_is_nil,
    handle_is_number, handle_is_pair, handle_is_set, handle_is_set_mut, handle_is_string,
    handle_is_struct, handle_is_struct_mut, handle_is_symbol, handle_length, handle_ne, handle_not,
    handle_type_of,
};

#[cfg(test)]
mod tests;
