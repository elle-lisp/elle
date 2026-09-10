// audited: 2026-09-10
// docs/impl/jit.md
//! Cranelift JIT compilation of LIR functions, and the types every stage of it
//! shares.
//!
//! ## Architecture
//!
//! ```text
//! LirFunction -> JitCompiler -> Cranelift IR -> Native code -> JitCode
//! ```
//!
//! A polymorphic or yielding function compiles like any other — the runtime
//! dispatch helper handles an arbitrary callable, and a callee that suspends
//! leaves through the yield side-exit. What `JitCompiler::compile` refuses is
//! listed there.
//!
//! ## Calling Convention
//!
//! JIT-compiled functions use this calling convention:
//!
//! ```ignore
//! type JitFn = unsafe extern "C" fn(
//!     env: *const Value,      // closure environment (captures array)
//!     args: *const Value,     // arguments array
//!     nargs: u32,             // number of arguments
//!     vm: *mut VM,            // pointer to VM (for globals, function calls)
//!     self_tag: u64,          // the executing closure's own Value, tag half
//!     self_payload: u64,      // and its payload half
//! ) -> Value;
//! ```
//!
//! The last two parameters carry the executing closure as a `Value`, which is
//! what makes the self-tail-call optimization possible: a tail call compares its
//! callee against that pair, and on a match updates the argument variables and
//! jumps to the loop header instead of calling `elle_jit_tail_call`. So a
//! self-recursive tail call is a native loop.

mod calls;
mod code;
mod compiler;
mod data;
pub(crate) mod dispatch;
mod fastpath;
#[allow(dead_code)]
mod group;
mod helpers;
pub(crate) mod registry;
mod runtime;
mod suspend;
mod translate;
mod value;
mod vtable;
pub(crate) mod worker;

pub use code::JitCode;
pub use compiler::{BatchMember, JitCompiler};
pub use dispatch::{TAIL_CALL_SENTINEL, YIELD_SENTINEL};
pub use value::JitValue;
pub use worker::{JIT_COMPILE_NS, JIT_COMPILE_TASKS};

use std::fmt;

/// The capability bundle threaded to the JIT intrinsic fast-path helpers
/// (`elle_jit_put`/`del`/`has`/`push`/`string_push`/`bytes_push`/`freeze`/`thaw`).
///
/// Those helpers run the same `PrimFn` bodies as the interpreter, so they need a
/// VM-bearing `NativeCtx`. A JIT function always runs under one specific VM, and
/// threading it explicitly through the helper ABI keeps the VM dependency visible
/// in the signature — which lets two embedded instances coexist in one process,
/// each helper reaching its own instance's VM. The compiled function's prologue
/// (`compiler/translate.rs`) builds a `JitCtx` in a stack slot — holding the
/// driving VM, its 4th entry parameter — and the intrinsic emit sites thread its
/// address to each helper, which resolves the VM from it (docs/impl/region/ctx.md
/// "JIT intrinsic helpers reach the VM through a JitCtx").
///
/// `#[repr(C)]` with the VM at offset 0 so the prologue's raw `stack_store` of the
/// `vm` pointer lands on the `vm` field; the heap axis extends this bundle with a
/// heap capability, threaded the same way, with no further change to the helper ABI.
#[repr(C)]
pub(crate) struct JitCtx {
    vm: *mut crate::vm::VM,
}

// The prologue stores the VM pointer at offset 0 of the JitCtx stack slot; this
// pins that the `vm` field lives there (the coupling cranelift cannot type-check).
const _: () = assert!(
    std::mem::offset_of!(JitCtx, vm) == 0,
    "JIT prologue stores the driving VM at offset 0 of the JitCtx slot"
);

impl JitCtx {
    /// Build over the driving VM. Non-null by construction — a JIT function
    /// always runs under a VM. Test-only: in compiled code the prologue
    /// materializes the equivalent stack slot directly.
    #[cfg(test)]
    #[inline]
    pub(crate) fn new(vm: *mut crate::vm::VM) -> Self {
        debug_assert!(!vm.is_null(), "JitCtx requires a non-null VM");
        JitCtx { vm }
    }

    /// The driving VM pointer (non-null).
    #[inline]
    pub(crate) fn vm(&self) -> *mut crate::vm::VM {
        self.vm
    }
}

/// The two deferral channels one `TailCall` carries to `elle_jit_tail_call`: the
/// callee closure's own region, and the merged closure-cycle arena's static slot
/// (see `LirInstr::TailCall`). A frame-replacing tail call strands both releases,
/// and the activation that runs the callee takes them over
/// (docs/impl/region/relocate.md § "A channel built in compiled code hands its
/// release forward").
///
/// Both are `u32` at the C ABI and neither is the other, so they cross as one
/// named pair rather than as two bare arguments a call site could swap.
#[derive(Clone, Copy)]
pub(crate) struct TailDeferrals {
    /// 1 when the new activation takes over the callee closure's own release.
    pub(crate) callee: u32,
    /// The merged arena's static slot, or 0 for none (`StaticRegion` is nonzero,
    /// so 0 is never a real slot).
    pub(crate) arena_slot: u32,
}

impl TailDeferrals {
    /// The pair a call site carries no deferral on: the spliced tail call, which
    /// carries neither channel on either tier, and the self-tail-call loop, which
    /// replaces no frame.
    pub(crate) const NONE: TailDeferrals = TailDeferrals {
        callee: 0,
        arena_slot: 0,
    };

    /// Read the pair off a `LirInstr::TailCall`'s own fields.
    pub(crate) fn of(
        defer_callee_release: bool,
        deferred_release_slot: Option<crate::hir::region::StaticRegion>,
    ) -> Self {
        TailDeferrals {
            callee: u32::from(defer_callee_release),
            arena_slot: deferred_release_slot.map_or(0, |s| s.get()),
        }
    }
}

/// JIT compilation error
#[derive(Debug, Clone)]
pub enum JitError {
    /// Instruction not supported by JIT
    UnsupportedInstruction(String),
    /// Function has polymorphic signal
    Polymorphic,
    /// Function has yielding signal (rejected by batch compilation only)
    Yielding,
    /// Cranelift compilation failed
    CompilationFailed(String),
    /// Invalid LIR structure
    InvalidLir(String),
}

impl fmt::Display for JitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JitError::UnsupportedInstruction(name) => {
                write!(f, "JIT: unsupported instruction: {}", name)
            }
            JitError::Polymorphic => write!(f, "JIT: function has polymorphic signal"),
            JitError::Yielding => write!(f, "JIT: yielding functions cannot be batch-compiled"),
            JitError::CompilationFailed(msg) => write!(f, "JIT compilation failed: {}", msg),
            JitError::InvalidLir(msg) => write!(f, "JIT: invalid LIR: {}", msg),
        }
    }
}

impl std::error::Error for JitError {}

/// Record of a closure that was rejected from JIT compilation.
/// One entry per closure template, deduplicated by bytecode pointer.
#[derive(Debug, Clone)]
pub struct JitRejectionInfo {
    /// Function name (from `LirFunction.name`), if available.
    pub name: Option<String>,
    /// Why the JIT rejected this closure.
    pub reason: JitError,
    /// Pin for the code object whose bytecode this rejection is keyed by
    /// (docs/impl/jit.md § "Cache identity"): while the entry lives, the
    /// address cannot be reused by a different function, so the negative
    /// cache can never wrongly block a new function from compiling.
    _pin: Option<crate::value::ClosureTemplate>,
}

impl JitRejectionInfo {
    /// Build a rejection record pinning the code object it is keyed by. `pin`
    /// is `None` only when the submission's pin was already lost (a worker
    /// result with no matching pending entry).
    pub fn new(reason: JitError, pin: Option<crate::value::ClosureTemplate>) -> Self {
        JitRejectionInfo {
            name: None,
            reason,
            _pin: pin,
        }
    }
}
