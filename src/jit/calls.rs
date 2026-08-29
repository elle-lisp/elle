//! Function call dispatch helpers for JIT-compiled code.
//!
//! These `extern "C"` functions handle calling Elle closures, native functions,
//! and parameters from JIT-compiled code. They also include the sentinels,
//! yield/call-site metadata types, and the environment-building utility used
//! by the interpreter fallback paths.

use crate::jit::value::{JitValue, TAIL_CALL_SENTINEL_JV, YIELD_SENTINEL_JV};
use crate::signals::dispatch::{classify, SignalAction};
use crate::value::fiber::{SignalBits, MAX_CALL_DEPTH, SIG_ERROR, SIG_HALT};
use crate::value::Value;

mod callops;
pub use callops::*;

// =============================================================================
// Sentinels and Metadata Types
// =============================================================================

/// Sentinel `JitValue` indicating a pending tail call.
/// Uses a tag value that cannot be a valid Value tag (> TAG_THREAD = 33).
pub const TAIL_CALL_SENTINEL: JitValue = TAIL_CALL_SENTINEL_JV;

/// Sentinel `JitValue` indicating a JIT function yielded (side-exited).
/// The caller checks for this after a JIT call and propagates the yield.
/// fiber.signal and fiber.suspended are already set by the JIT yield helper.
pub const YIELD_SENTINEL: JitValue = YIELD_SENTINEL_JV;

/// Metadata for a single yield point in JIT-compiled code.
/// Stored in `JitCode.yield_points`, indexed by yield point index.
/// Read by `elle_jit_yield` runtime helper.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct YieldPointMeta {
    /// Bytecode IP to resume at (matches the interpreter's SuspendedFrame.ip)
    pub resume_ip: usize,
    /// Number of spilled values that constitute the operand stack.
    pub num_spilled: u16,
    /// Number of locally-defined variable slots (excludes params).
    pub num_locals: u16,
    /// Number of function parameters (spilled from arg_var_base).
    pub num_params: u16,
}

/// Metadata for a single call site in JIT-compiled code.
/// Stored in `JitCode.call_sites`, indexed by call site index.
/// Read by `elle_jit_yield_through_call` runtime helper.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct CallSiteMeta {
    /// Bytecode IP to resume at (matches the interpreter's SuspendedFrame.ip)
    pub resume_ip: usize,
    /// Number of spilled operand stack values.
    pub num_spilled: u16,
    /// Number of locally-defined variable slots (excludes params).
    pub num_locals: u16,
    /// Number of function parameters (spilled from arg_var_base).
    pub num_params: u16,
}

// =============================================================================
// Primitive Signal Handling (for JIT dispatch)
// =============================================================================

/// Handle signal bits from a primitive call in JIT context.
///
/// Returns a `JitValue` for the result.
fn jit_handle_primitive_signal(vm: &mut crate::vm::VM, bits: SignalBits, value: Value) -> JitValue {
    match classify(bits, &value) {
        SignalAction::Ok => JitValue::from_value(value),
        SignalAction::Resume => vm.handle_fiber_resume_signal_jit(value),
        SignalAction::Propagate => vm.handle_fiber_propagate_signal_jit(value),
        SignalAction::Abort => vm.handle_fiber_abort_signal_jit(value),
        SignalAction::Query => {
            // A query answered from JIT-compiled code (no compiler result slot):
            // build a boundary ctx so the answer is born on a fresh region of the
            // VM's heap, freed value-based by the consumer.
            let mut ctx = crate::primitives::ctx::Alloc::boundary(unsafe { &mut *vm.heap_ptr });
            let (sig, result) = vm.dispatch_query(&mut ctx, value);
            if sig.intersects(SIG_ERROR) {
                vm.fiber.signal = Some((sig, result));
                JitValue::nil()
            } else {
                JitValue::from_value(result)
            }
        }
        SignalAction::Error | SignalAction::Halt => {
            vm.fiber.signal = Some((bits, value));
            JitValue::nil()
        }
        SignalAction::Suspend => {
            // Rule-5 suspend-escape retain — the exact mirror of the
            // interpreter's `handle_primitive_signal` Suspend arm
            // (src/vm/signal.rs). The yielded value escapes into `fiber.signal`,
            // where the scheduler reads it (e.g. an `IoRequest` whose read buffer
            // becomes the resume result, co-located in one region). Without this
            // incref the region's only reference is dropped when the resume
            // consumer's `DecrefValueRegion` fires, and the scheduler's release
            // of the same region double-frees it (the redis eager/adaptive-JIT
            // crash; tests/elle/region-jit-io-suspend-uaf.lisp). `region_of`, NOT
            // `result_region_of`: the escaping value's own region is the one held
            // live across the suspend.
            let heap = unsafe { &mut *vm.heap_ptr };
            let r = crate::value::arena::region_of(heap, value);
            crate::value::arena::incref_for_escape(
                heap,
                r,
                crate::value::arena::EscapeSite::SuspendEscape,
            );
            // …and the same arm's park classification: this primitive never
            // returns, so the resume value stands in for its result and the
            // delivery owes the reference the missing `Return` mint would have
            // carried (docs/impl/region/owner.md § "A delivery into a replayed
            // frame carries one owning reference").
            vm.fiber.resume_value_unfunded = true;
            vm.fiber.signal = Some((bits, value));
            YIELD_SENTINEL
        }
    }
}

/// Capability denial for a native called from JIT-compiled code.
///
/// The interpreter gates every native call on the fiber's withheld capabilities
/// (`call_inner`, src/vm/call/inner.rs: `def.signal ∩ withheld ∩ CAP_MASK`) and,
/// when they overlap, denies the call instead of running it. The JIT native
/// dispatch path (`elle_jit_call`) must apply the identical gate — otherwise a
/// JIT-compiled fiber body reaches a withheld primitive (e.g. an `:io` `port/write`
/// on an `:io`-denied fiber), runs it, and suspends on the raw effect request
/// rather than the denial payload, so `fiber/value` reads the wrong value.
///
/// This mirrors the `SignalAction::Suspend` arm above: build the
/// `{:error :capability-denied …}` payload, retain its region for the escape into
/// `fiber.signal` (read later via `fiber/value`, after control has left this
/// fiber — without the retain the resumer's `DecrefValueRegion` frees it under the
/// reader), set the signal, and return `YIELD_SENTINEL` so the JIT suspend
/// machinery parks the frame — the analogue of the interpreter's
/// `handle_capability_denial` frame save.
pub(crate) fn jit_capability_denial(
    vm: &mut crate::vm::VM,
    def: &'static crate::primitives::def::PrimitiveDef,
    blocked: SignalBits,
    args: &[Value],
) -> JitValue {
    let payload = {
        let mut ctx = crate::primitives::ctx::Alloc::new(unsafe { &mut *vm.heap_ptr });
        crate::vm::VM::build_denial_payload(&mut ctx, def, blocked, args)
    };
    let heap = unsafe { &mut *vm.heap_ptr };
    let r = crate::value::arena::region_of(heap, payload);
    crate::value::arena::incref_for_escape(heap, r, crate::value::arena::EscapeSite::SuspendEscape);
    // The denied primitive never runs, so the mediating parent's resume value
    // stands in for its result — the same park classification the interpreter's
    // `handle_capability_denial` records, and the same left-over payload reference
    // for the resume to release (`Fiber::denial_payload`).
    vm.fiber.resume_value_unfunded = true;
    vm.fiber.denial_payload = Some(payload);
    vm.fiber.signal = Some((blocked, payload));
    YIELD_SENTINEL
}

// =============================================================================
// Exception Checking
// =============================================================================

/// Check if a terminal signal is pending on the VM (error or halt).
/// Returns TRUE if one is set, FALSE otherwise.
///
/// Uses bitwise containment (`contains`) rather than exact equality,
/// because signals can be compound (e.g. `SIG_ERROR | SIG_IO`).
#[no_mangle]
pub extern "C" fn elle_jit_has_exception(vm: u64) -> JitValue {
    let vm = unsafe { &*(vm as *const crate::vm::VM) };
    JitValue::bool_val(
        vm.fiber
            .signal
            .as_ref()
            .is_some_and(|(b, _)| b.intersects(SIG_ERROR) || b.intersects(SIG_HALT)),
    )
}

// =============================================================================
// Function Calls
// =============================================================================

/// Reinterpret a JIT args pointer as a `&[Value]` slice.
///
/// The JIT passes a `*const Value` (16 bytes each). Handles the null-pointer
/// case when `nargs` is 0.
#[inline]
pub(crate) fn args_ptr_to_value_slice(args_ptr: *const Value, nargs: u32) -> &'static [Value] {
    if nargs == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(args_ptr, nargs as usize) }
    }
}

/// Debug-build guard at the compiled-call boundary: classify the callee's env
/// backing through the region-of funnel, whose generation check panics with
/// free-log attribution when the backing page's region was freed
/// (docs/impl/region/generations.md § "Region generations"). Compiled code
/// reads the env by raw pointer — no funnel, no check — so a stale env
/// crosses this boundary silently and detonates later at an unattributed
/// load in native code. Classifying here names the call boundary instead.
/// Release builds skip it: the funnel walk costs a page-header probe per
/// compiled call. Called by both compiled-call entries: `VM::call_jit`
/// (interpreter→JIT, src/vm/jit_entry.rs) and the JIT-to-JIT dispatch in
/// `calls/callops.rs`.
#[inline]
pub(crate) fn debug_check_env_backing(
    heap: &crate::value::fiberheap::FiberHeap,
    closure: &crate::value::Closure,
) {
    if cfg!(debug_assertions) && !closure.env.is_empty() {
        // The stale case panics inside `region_of_ptr`; the id is unused.
        let _ = heap.region_of_ptr(closure.env.as_ptr() as *const ());
    }
}

/// Hand a JIT-to-JIT (or SCC direct) callee one `CallArgument` owning reference
/// per non-captured FIXED param, mirroring `VM::populate_env`/`push_param`
/// (own_params=true) for the path where no interpreter env is built (the callee
/// runs as compiled code, reading args by pointer and releasing each owned param
/// via `DecrefValueRegion`). Position-aware:
///   - non-captured fixed params  → incref (the callee will release them);
///   - captured params (cell-owned) → NO incref (the cell's auto-incref owns the
///     wrapped value, balanced by `DecrefCellRegion`);
///   - rest args (collected into the rest list by the callee prologue) → NO
///     incref (the cons construction's `alloc_obj` scan increfs each element).
///
/// `result_region_of` matches the region the callee's `DecrefValueRegion`
/// targets; an immediate (no region) no-ops. Also used by the interpreter→JIT
/// entry `VM::call_jit` (`src/vm/jit_entry.rs`), which likewise hands args by
/// pointer to compiled code with no env build.
pub(crate) fn incref_owned_call_args(
    heap: &mut crate::value::fiberheap::FiberHeap,
    closure: &crate::value::Closure,
    args: &[Value],
) {
    use crate::value::arena::{incref_for_escape, result_region_of, EscapeSite};
    let mask = closure.template.capture_params_mask;
    let mut incref_fixed = |upto: usize| {
        for (i, &arg) in args.iter().take(upto).enumerate() {
            let captured = i < 64 && (mask & (1 << i)) != 0;
            if !captured {
                let r = result_region_of(heap, arg);
                incref_for_escape(heap, r, EscapeSite::CallArgument);
            }
        }
    };
    match closure.template.arity {
        crate::value::Arity::Exact(_) | crate::value::Arity::Range(_, _) => {
            incref_fixed(args.len());
        }
        crate::value::Arity::AtLeast(_) => {
            // Mirror `populate_env`'s `provided_fixed` boundary: only the args
            // that fill non-rest slots are owned params; the rest are collected.
            let fixed_slots = closure.template.num_params - 1;
            let collects_keywords = matches!(
                closure.template.vararg_kind,
                crate::hir::VarargKind::Struct | crate::hir::VarargKind::StrictStruct(_)
            );
            let provided_fixed = if collects_keywords {
                let min = closure.template.arity.fixed_params();
                let mut count = args.len().min(min);
                while count < fixed_slots && count < args.len() {
                    if args[count].as_keyword_name().is_some() {
                        break;
                    }
                    count += 1;
                }
                count
            } else {
                args.len().min(fixed_slots)
            };
            incref_fixed(provided_fixed);
        }
    }
}

#[cfg(test)]
mod tests;
