// audited: 2026-09-06
// docs/impl/wasm.md
//! The numbers the emitted module and the host agree on: import indices, the
//! reserved slots of linear memory, and the data-operation codes.
//!
//! Each is written on one side and read on the other, so a change here is a
//! change to both. The import indices follow the declaration order in
//! `emit_types_and_imports`; the data-operation codes follow
//! `dispatch_data_op` (src/wasm/linker/dataop.rs).

// Host function import indices (in order of declaration)
pub(in crate::wasm) const FN_RT_CALL: u32 = 1;
pub(in crate::wasm) const FN_RT_LOAD_CONST: u32 = 2;
pub(in crate::wasm) const FN_RT_DATA_OP: u32 = 3;
pub(in crate::wasm) const FN_RT_MAKE_CLOSURE: u32 = 4;
pub(in crate::wasm) const FN_RT_PUSH_PARAM: u32 = 5;
pub(in crate::wasm) const FN_RT_POP_PARAM: u32 = 6;
pub(in crate::wasm) const FN_RT_PREPARE_TAIL_CALL: u32 = 7;
pub(in crate::wasm) const FN_RT_YIELD: u32 = 8;
pub(in crate::wasm) const FN_RT_GET_RESUME_VALUE: u32 = 9;
pub(in crate::wasm) const FN_RT_LOAD_SAVED_REG: u32 = 10;

// First non-imported function index
pub(in crate::wasm) const FN_ENTRY: u32 = 11;

// Linear memory layout
pub(in crate::wasm) const ARGS_BASE: i32 = 256;

/// Reserved 8-byte slot at the base of linear memory holding the `SignalBits` a
/// compiled function raised.
///
/// A compiled function answers on two channels, and both must be read. The
/// `status` word it returns says whether it suspended; the signal it raised
/// goes here. Reading only `status` reports a failed primitive as a successful
/// return of the error value — see `store::take_raised_signal`.
pub(in crate::wasm) const SIGNAL_SLOT: i32 = 0;

/// Reserved 16-byte slot in linear memory holding the **executing closure**
/// (tag at `SELF_SLOT`, payload at `SELF_SLOT + 8`) — the WASM analogue of the
/// interpreter's `Fiber::current_closure` and the JIT's `self_tag_payload`. A
/// `LoadSelf` reads it; the host writes it at every closure entry
/// (`call_wasm_closure` / `call_precached_closure` / `rt_prepare_tail_call` / the
/// tiered self-dispatch), restores it around a nested call (shared linear memory),
/// and carries it across suspend/resume on the `WasmSuspensionFrame`. Lives in the
/// free scratch below `ARGS_BASE` (the signal occupies `[0, 8)`; args start at 256).
pub(in crate::wasm) const SELF_SLOT: i32 = 16;

/// Data operation codes for rt_data_op.
///
/// These must stay in sync with `dispatch_data_op`
/// (src/wasm/linker/dataop.rs).
#[repr(i32)]
#[derive(Clone, Copy)]
pub(in crate::wasm) enum DataOp {
    Pair = 0,
    First = 1,
    Rest = 2,
    FirstDestructure = 3,
    RestDestructure = 4,
    FirstOrNil = 5,
    RestOrNil = 6,
    MakeArray = 7,
    MakeCapture = 8,
    LoadCapture = 9,
    StoreCapture = 10,
    // 11 = MakeString (unused)
    ArrayRefDestructure = 12,
    ArraySliceFrom = 13,
    StructGetOrNil = 14,
    StructGetDestructure = 15,
    ArrayExtend = 16,
    ArrayPush = 17,
    ArrayLen = 18,
    ArrayRefOrNil = 19,
    StructRest = 20,
    IntToFloat = 21,
    FloatToInt = 22,
    IntrTypeOf = 23,
    IntrLength = 24,
    IntrGetOp = 25,
    IntrPutOp = 26,
    IntrDelOp = 27,
    IntrHasOp = 28,
    IntrPopOp = 29,
    IntrFreezeOp = 30,
    IntrThawOp = 31,
    IntrIdenticalOp = 32,
    IntrPushOp = 33,
    IntrStringPushOp = 34,
    IntrBytesPushOp = 35,
    MatchFail = 36,
}

// The i32 form each emitter site passes to `rt_data_op`.
pub(in crate::wasm) const OP_CONS: i32 = DataOp::Pair as i32;
pub(in crate::wasm) const OP_CAR: i32 = DataOp::First as i32;
pub(in crate::wasm) const OP_CDR: i32 = DataOp::Rest as i32;
pub(in crate::wasm) const OP_CAR_DESTRUCTURE: i32 = DataOp::FirstDestructure as i32;
pub(in crate::wasm) const OP_MATCH_FAIL: i32 = DataOp::MatchFail as i32;
pub(in crate::wasm) const OP_CDR_DESTRUCTURE: i32 = DataOp::RestDestructure as i32;
pub(in crate::wasm) const OP_CAR_OR_NIL: i32 = DataOp::FirstOrNil as i32;
pub(in crate::wasm) const OP_CDR_OR_NIL: i32 = DataOp::RestOrNil as i32;
pub(in crate::wasm) const OP_MAKE_ARRAY: i32 = DataOp::MakeArray as i32;
pub(in crate::wasm) const OP_MAKE_CAPTURE: i32 = DataOp::MakeCapture as i32;
pub(in crate::wasm) const OP_LOAD_CAPTURE: i32 = DataOp::LoadCapture as i32;
pub(in crate::wasm) const OP_STORE_CAPTURE: i32 = DataOp::StoreCapture as i32;
pub(in crate::wasm) const OP_ARRAY_REF_DESTRUCTURE: i32 = DataOp::ArrayRefDestructure as i32;
pub(in crate::wasm) const OP_ARRAY_SLICE_FROM: i32 = DataOp::ArraySliceFrom as i32;
pub(in crate::wasm) const OP_STRUCT_GET_OR_NIL: i32 = DataOp::StructGetOrNil as i32;
pub(in crate::wasm) const OP_STRUCT_GET_DESTRUCTURE: i32 = DataOp::StructGetDestructure as i32;
pub(in crate::wasm) const OP_ARRAY_EXTEND: i32 = DataOp::ArrayExtend as i32;
pub(in crate::wasm) const OP_ARRAY_PUSH: i32 = DataOp::ArrayPush as i32;
pub(in crate::wasm) const OP_ARRAY_LEN: i32 = DataOp::ArrayLen as i32;
pub(in crate::wasm) const OP_ARRAY_REF_OR_NIL: i32 = DataOp::ArrayRefOrNil as i32;
pub(in crate::wasm) const OP_STRUCT_REST: i32 = DataOp::StructRest as i32;
pub(in crate::wasm) const OP_INT_TO_FLOAT: i32 = DataOp::IntToFloat as i32;
pub(in crate::wasm) const OP_FLOAT_TO_INT: i32 = DataOp::FloatToInt as i32;
pub(in crate::wasm) const OP_TYPE_OF: i32 = DataOp::IntrTypeOf as i32;
pub(in crate::wasm) const OP_LENGTH: i32 = DataOp::IntrLength as i32;
pub(in crate::wasm) const OP_INTR_GET: i32 = DataOp::IntrGetOp as i32;
pub(in crate::wasm) const OP_INTR_PUT: i32 = DataOp::IntrPutOp as i32;
pub(in crate::wasm) const OP_INTR_DEL: i32 = DataOp::IntrDelOp as i32;
pub(in crate::wasm) const OP_INTR_HAS: i32 = DataOp::IntrHasOp as i32;
pub(in crate::wasm) const OP_INTR_POP: i32 = DataOp::IntrPopOp as i32;
pub(in crate::wasm) const OP_INTR_FREEZE: i32 = DataOp::IntrFreezeOp as i32;
pub(in crate::wasm) const OP_INTR_THAW: i32 = DataOp::IntrThawOp as i32;
pub(in crate::wasm) const OP_INTR_IDENTICAL: i32 = DataOp::IntrIdenticalOp as i32;
pub(in crate::wasm) const OP_INTR_PUSH: i32 = DataOp::IntrPushOp as i32;
pub(in crate::wasm) const OP_INTR_STRING_PUSH: i32 = DataOp::IntrStringPushOp as i32;
pub(in crate::wasm) const OP_INTR_BYTES_PUSH: i32 = DataOp::IntrBytesPushOp as i32;
