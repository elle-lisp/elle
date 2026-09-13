// audited: 2026-09-13
// docs/impl/jit.md
//! `RuntimeHelpers`: one pre-declared Cranelift `FuncId` per `extern "C"`
//! runtime helper, so a translator emits a call without re-declaring.
//!
//! The two halves each live in a submodule: `symbols` registers every
//! `elle_jit_*` name with the JIT linker, and `helpers` declares each one's
//! signature into the module and builds this struct.
//!
//! ## Calling convention for Values
//!
//! Values are passed and returned as TWO `I64` Cranelift arguments: (tag, payload).
//! A "Value parameter" = two consecutive I64 params.
//! A "Value return" = two consecutive I64 return values.
//!
//! Helper arity table (counting Value params as 2 each):
//!   value_unary: (tag, payload) -> (tag, payload)           = 2 params, 2 returns
//!   value_binary: (atag, apay, btag, bpay) -> (tag, payload) = 4 params, 2 returns
//!   value_unary_vm: (tag, payload, vm) -> (tag, payload)     = 3 params, 2 returns
//!   value_binary_vm: (atag, apay, btag, bpay, vm) -> (tag, payload) = 5 params, 2 returns
//!   call: (ftag, fpay, args_ptr, nargs, vm) -> (tag, payload) = 5 params, 2 returns

use cranelift_module::FuncId;

mod helpers;
mod symbols;
pub(crate) use helpers::*;
pub(crate) use symbols::register_symbols;

/// Pre-declared runtime helper function IDs.
///
/// Each field maps to a `#[no_mangle] extern "C"` function in `runtime.rs`
/// or `dispatch.rs` / `data.rs` / `suspend.rs`. The IDs are declared in the
/// JITModule at construction time so that `FunctionTranslator` can reference
/// them without re-declaring on every function compilation.
pub(crate) struct RuntimeHelpers {
    pub(crate) add: FuncId,
    pub(crate) sub: FuncId,
    pub(crate) mul: FuncId,
    pub(crate) div: FuncId,
    pub(crate) rem: FuncId,
    pub(crate) bit_and: FuncId,
    pub(crate) bit_or: FuncId,
    pub(crate) bit_xor: FuncId,
    pub(crate) shl: FuncId,
    pub(crate) shr: FuncId,
    pub(crate) neg: FuncId,
    pub(crate) not: FuncId,
    pub(crate) bit_not: FuncId,
    pub(crate) int_to_float: FuncId,
    pub(crate) float_to_int: FuncId,
    pub(crate) eq: FuncId,
    pub(crate) ne: FuncId,
    pub(crate) lt: FuncId,
    pub(crate) le: FuncId,
    pub(crate) gt: FuncId,
    pub(crate) ge: FuncId,
    pub(crate) pair: FuncId,
    pub(crate) first: FuncId,
    pub(crate) rest: FuncId,
    pub(crate) make_array: FuncId,
    /// Materialize a heap literal (string, or quoted compound data) into the
    /// current alloc region from a JIT-code-owned `ConstTemplate`. See
    /// `dispatch::elle_jit_materialize_const`.
    pub(crate) materialize_const: FuncId,
    pub(crate) is_nil: FuncId,
    pub(crate) is_pair: FuncId,
    pub(crate) is_array: FuncId,
    pub(crate) is_array_mut: FuncId,
    pub(crate) is_struct: FuncId,
    pub(crate) is_struct_mut: FuncId,
    pub(crate) is_set: FuncId,
    pub(crate) is_set_mut: FuncId,
    pub(crate) first_or_nil: FuncId,
    pub(crate) rest_or_nil: FuncId,
    pub(crate) array_len: FuncId,
    pub(crate) array_ref_or_nil: FuncId,
    pub(crate) match_fail: FuncId,
    pub(crate) first_destructure: FuncId,
    pub(crate) rest_destructure: FuncId,
    pub(crate) array_ref_destructure: FuncId,
    pub(crate) array_slice_from: FuncId,
    pub(crate) struct_get_or_nil: FuncId,
    pub(crate) struct_get_destructure: FuncId,
    pub(crate) struct_rest: FuncId,
    pub(crate) check_signal_bound: FuncId,
    pub(crate) array_push: FuncId,
    pub(crate) array_extend: FuncId,
    pub(crate) push_param_frame: FuncId,
    #[allow(dead_code)]
    pub(crate) is_truthy: FuncId,
    pub(crate) make_capture: FuncId,
    /// Capture cell minted into its OWN fresh per-execution region (JIT-prologue
    /// env path; mirrors the interpreter's `env_value_region`). See
    /// `dispatch::elle_jit_make_capture_owned`.
    pub(crate) make_capture_owned: FuncId,
    /// Variadic rest list with per-cons fresh regions (JIT-prologue env path;
    /// mirrors the interpreter's `args_to_list`). See
    /// `dispatch::elle_jit_collect_rest_list`.
    pub(crate) collect_rest_list: FuncId,
    pub(crate) load_capture_cell: FuncId,
    pub(crate) load_capture: FuncId,
    pub(crate) store_capture_cell: FuncId,
    pub(crate) store_capture: FuncId,
    pub(crate) call: FuncId,
    pub(crate) tail_call: FuncId,
    pub(crate) has_exception: FuncId,
    pub(crate) pop_param_frame: FuncId,
    pub(crate) call_array: FuncId,
    pub(crate) tail_call_array: FuncId,
    #[allow(dead_code)] // infrastructure for future JIT MakeClosure support
    pub(crate) make_closure: FuncId,
    pub(crate) jit_yield: FuncId,
    pub(crate) jit_yield_through_call: FuncId,
    pub(crate) has_signal: FuncId,
    #[allow(dead_code)] // JIT region infrastructure — wired incrementally
    pub(crate) region_enter: FuncId,
    #[allow(dead_code)]
    pub(crate) region_exit: FuncId,
    #[allow(dead_code)]
    pub(crate) region_exit_call: FuncId,
    #[allow(dead_code)]
    pub(crate) region_rotate: FuncId,
    pub(crate) incref_region: FuncId,
    pub(crate) decref_region: FuncId,
    pub(crate) decref_value_region: FuncId,
    pub(crate) decref_cell_region: FuncId,
    pub(crate) incref_value_region: FuncId,
    /// Link a child value's region as Owned by a parent value's region — the
    /// `AdoptRegion` instruction's JIT helper, mirroring `handle_adopt_region`.
    pub(crate) adopt_region: FuncId,
    /// Link a child value's region as Owned by a parent value's region using
    /// `region_of` (NOT `result_region_of`) — the `AdoptCellRegion` instruction's
    /// JIT helper, mirroring `handle_adopt_cell_region`. Adopts a capture cell's
    /// OWN region (never unwrapped).
    pub(crate) adopt_cell_region: FuncId,
    /// Adopt a child value's region into the current activation's owner node —
    /// the `AdoptIntoActivation` instruction's JIT helper, mirroring
    /// `handle_adopt_into_activation`.
    pub(crate) adopt_into_activation: FuncId,
    /// Free the current activation's owner node at the compiled `Return` path —
    /// the JIT twin of the interpreter trampoline's clean-break release.
    pub(crate) release_activation_dues: FuncId,
    /// Run the releases this compiled activation still owed at an **error** exit
    /// — the compiled entry to the interpreter's abandoned-frame walk.
    pub(crate) release_abandoned_frame: FuncId,
    /// Free a co-owned region group as one unit — the `FreeRegionGroup`
    /// instruction's JIT helper, mirroring `handle_free_region_group`.
    pub(crate) free_region_group: FuncId,
    pub(crate) push_region_map: FuncId,
    pub(crate) pop_region_map: FuncId,
    pub(crate) resolve_alloc_region: FuncId,
    /// The mint-or-reuse variant of `resolve_alloc_region`, selected at emit time
    /// for a slot in `LirFunction.merged_slots` (builder-idiom merge;
    /// docs/impl/region/merging.md § Merging).
    pub(crate) resolve_alloc_region_merged: FuncId,
    #[allow(dead_code)]
    pub(crate) rotate_pools: FuncId,
    #[allow(dead_code)]
    pub(crate) incref: FuncId,
    #[allow(dead_code)]
    pub(crate) decref: FuncId,
    // New intrinsic helpers
    pub(crate) is_empty: FuncId,
    pub(crate) is_bool: FuncId,
    pub(crate) is_int: FuncId,
    pub(crate) is_float: FuncId,
    pub(crate) is_string: FuncId,
    pub(crate) is_keyword: FuncId,
    pub(crate) is_symbol_check: FuncId,
    pub(crate) is_bytes: FuncId,
    pub(crate) is_box: FuncId,
    pub(crate) is_closure: FuncId,
    pub(crate) is_fiber: FuncId,
    pub(crate) type_of: FuncId,
    pub(crate) length: FuncId,
    pub(crate) get: FuncId,
    pub(crate) put: FuncId,
    pub(crate) del: FuncId,
    pub(crate) has: FuncId,
    pub(crate) intr_push: FuncId,
    pub(crate) intr_string_push: FuncId,
    pub(crate) intr_bytes_push: FuncId,
    pub(crate) pop: FuncId,
    pub(crate) freeze: FuncId,
    pub(crate) thaw: FuncId,
    pub(crate) identical: FuncId,
}
