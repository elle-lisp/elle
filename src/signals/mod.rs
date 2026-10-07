// audited: 2026-10-06
//! Signal type for tracking which signals a function may emit.
//!
//! docs/signals/protocol.md
//! docs/signals/inference.md
//!
//! Signals are signal-bits-based: they track which signals a function
//! might emit (error, yield, debug, ffi, user-defined) and which
//! parameter indices propagate their callee's signals (for higher-order
//! functions like map/filter/fold).
//!
//! ## Compile-time vs. runtime signal representation
//!
//! `Signal` (this module) is a **compile-time** type used during HIR analysis
//! and LIR lowering. Its `propagates` field is a bitmask of parameter indices
//! whose signals flow through the function — this is needed to infer the signal
//! of a call site based on its arguments. `SignalBits` (in `value/fiber/signalbits.rs`) is
//! the **runtime** representation: a flat bitmask stored on closures and used by
//! the VM and JIT for dispatch. These are intentionally separate types serving
//! different phases. The `propagates` field has no runtime analogue. Do not
//! attempt to unify them.

mod constructors;
pub mod dispatch;
pub mod registry;

use crate::value::fiber::SignalBits;
use std::fmt;

// ---------------------------------------------------------------------------
// Signal constants — canonical definitions
// ---------------------------------------------------------------------------
//
// These are the semantic signal definitions for the signal system. They live
// here because the signal registry is the semantic owner; fiber.rs
// is a runtime data structure that consumes them.
//
// Signal bit partitioning:
//
//   Bit  0:     Error - exception, abort
//   Bit  1:     Yield - cooperative suspension
//   Bit  2:     Debug - breakpoint or trace
//   Bit  3:     Resume - run a suspended fiber (VM-internal)
//   Bit  4:     FFI — calls foreign code
//   Bit  5:     Propagate — propagate caught signal (VM-internal)
//   Bit  6:     Unused (the abort signal, SIG_ABORT, is Error + Terminal)
//   Bit  7:     Query — read VM state without fiber swap (VM-internal)
//   Bit  8:     Halt — graceful VM termination with return value
//   Bit  9:     IO — I/O request to scheduler
//   Bit  10:    Terminal — non-resumable signal
//   Bit  11:    Exec — subprocess capability (no backend of its own; see below)
//   Bit  12:    Fuel — instruction budget exhaustion
//   Bit  13:    Switch - fiber switch trampoline
//   Bit  14:    Wait - structured concurrency wait request
//   Bit  15:    GPU
//   Bit  16:    OsSignal — POSIX signal send/raise capability (see below)
//   Bit  17:    Fs — filesystem capability (see below)
//   Bits 18-31: Runtime-reserved (future runtime signals)
//   Bits 32-63: User-defined signal types
//
// "Capability bit" says only that the bit selects no I/O backend: `SIG_IO` is
// what routes a request to the scheduler, and `:exec` rides alongside it rather
// than replacing it. It does NOT mean the bit is inert in a fiber mask. A mask
// catches a signal on any shared bit, so `|:exec|` catches a subprocess request
// exactly as `|:io|` does — see `SignalBits::covers`.

pub const SIG_OK: SignalBits = SignalBits::EMPTY; // no bits set = normal return
pub const SIG_ERROR: SignalBits = SignalBits::new(1 << 0); // exception / panic
pub const SIG_YIELD: SignalBits = SignalBits::new(1 << 1); // cooperative suspension
pub const SIG_DEBUG: SignalBits = SignalBits::new(1 << 2); // breakpoint / trace
pub const SIG_RESUME: SignalBits = SignalBits::new(1 << 3); // fiber resumption (VM-internal)
pub const SIG_FFI: SignalBits = SignalBits::new(1 << 4); // calls foreign code
pub const SIG_PROPAGATE: SignalBits = SignalBits::new(1 << 5); // propagate caught signal (VM-internal)
pub const SIG_ABORT: SignalBits = SIG_ERROR.union(SIG_TERMINAL); // raise an error at a paused fiber's suspension point (VM-internal)
pub const SIG_QUERY: SignalBits = SignalBits::new(1 << 7); // VM state query (VM-internal)
pub const SIG_HALT: SignalBits = SignalBits::new(1 << 8); // graceful VM termination
pub const SIG_IO: SignalBits = SignalBits::new(1 << 9); // I/O request to scheduler
pub const SIG_TERMINAL: SignalBits = SignalBits::new(1 << 10); // terminal signal (non-resumable)
pub const SIG_EXEC: SignalBits = SignalBits::new(1 << 11); // subprocess capability (capability bit, not dispatch)
pub const SIG_FUEL: SignalBits = SignalBits::new(1 << 12); // instruction budget exhaustion
pub const SIG_SWITCH: SignalBits = SignalBits::new(1 << 13); // fiber switch trampoline (VM-internal)
pub const SIG_WAIT: SignalBits = SignalBits::new(1 << 14); // structured concurrency wait request
pub const SIG_GPU: SignalBits = SignalBits::new(1 << 15); // GPU hardware dispatch (capability bit)
pub const SIG_OS_SIGNAL: SignalBits = SignalBits::new(1 << 16); // POSIX signal send/raise (capability bit)
pub const SIG_FS: SignalBits = SignalBits::new(1 << 17); // filesystem access (capability bit, not dispatch)

/// The scheduler round trip: the dispatch bit that routes a request to the
/// I/O backend, and the error it may come back with. Every async primitive
/// carries these two; a capability-gated one adds its own bit on top, so the
/// base cannot drift between them.
///
/// `SIG_YIELD` is deliberately absent. The request does suspend its fiber, but
/// suspension follows from raising a signal at all — see
/// [`dispatch::is_suspending`] — not from that bit. `:yield` is the keyword a
/// generator's mask names, and a request that carried it would be caught by
/// every such mask on its way to the scheduler.
const IO_ROUND_TRIP: SignalBits = SIG_IO.union(SIG_ERROR);

/// VM-internal signal bits: signals the runtime raises for its own machinery
/// rather than for what a program does — the VM's dispatch, fuel metering, and
/// the stdlib scheduler's `:wait`. They lie outside `CAP_MASK`, so no fiber can
/// be denied them.
const VM_INTERNAL: SignalBits = SIG_RESUME
    .union(SIG_PROPAGATE)
    .union(SIG_QUERY)
    .union(SIG_TERMINAL)
    .union(SIG_FUEL)
    .union(SIG_SWITCH)
    .union(SIG_WAIT);

/// Pause bits: suspensions the VM injects at its own charge sites, under
/// whatever code happens to be running there. `:fuel` is the metering pause —
/// the interpreter raises it when a fiber's instruction budget runs out, and
/// the metering parent (`lib/process.lisp` preemption, a stepping debugger)
/// owns the resume.
///
/// A pause is the VM's action, not the paused code's behavior, so it is
/// exempt from squelch/attune enforcement — see [`squelched_bits`].
pub const SIG_PAUSE: SignalBits = SIG_FUEL;

/// The bits a `squelch`/`attune` boundary converts into a `signal-violation`
/// when a closure carrying `mask` produces `bits`. An empty result means the
/// signal crosses the boundary untouched.
///
/// This is the one predicate behind every enforcement site — the interpreter's
/// `VM::enforce_squelch` and the JIT's call, tail-call, and sentinel paths —
/// so the exemptions cannot drift apart between tiers.
///
/// Three classes never violate a boundary:
///
/// - `:error` and `:halt` are the escapes every boundary lets out, so a signal
///   carrying either passes whole.
/// - `:switch`, matched exactly, is the VM's fiber-switch trampoline. The exact
///   match keeps a user signal that merely rides alongside enforceable.
/// - The pause bits pass by subtraction rather than exempting the whole signal,
///   so a compound `|:fuel :log|` still violates a squelch of `:log`.
#[inline]
pub fn squelched_bits(bits: SignalBits, mask: SignalBits) -> SignalBits {
    if bits.intersects(SIG_ERROR) || bits.intersects(SIG_HALT) || bits == SIG_SWITCH {
        return SignalBits::EMPTY;
    }
    bits.intersection(mask).subtract(SIG_PAUSE)
}

/// Capability mask: all signals that user code can produce.
///
/// Defined as the complement of VM-internal bits within the 64-bit signal
/// space (bits 0-17 built-in, bits 18-31 runtime-reserved,
/// bits 32-63 user-defined). Used for capability enforcement (which
/// operations a fiber can be denied) and for static analysis (what an
/// unknown callee might emit).
pub const CAP_MASK: SignalBits = SignalBits::new(VM_INTERNAL.raw() ^ 0xFFFF_FFFF_FFFF_FFFF);

/// Signal classification for expressions and functions.
///
/// Two fields:
/// - `bits`: which signals this function itself might emit
/// - `propagates`: bitmask of parameter indices whose signals this
///   function propagates (bit i set = parameter i's signals flow through)
///
/// `Copy` and `const fn` constructors — no allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Signal {
    /// Signal bits this function itself might emit.
    pub bits: SignalBits,
    /// Bitmask of parameter indices whose signals this function propagates.
    /// Bit i set means this function may exhibit parameter i's signals.
    pub propagates: u32,
}

impl Default for Signal {
    fn default() -> Self {
        Signal::silent()
    }
}

// ── Predicates ──────────────────────────────────────────────────────
//
// Each predicate asks a specific question about capabilities.

impl Signal {
    /// Can this function suspend execution?
    /// Any signal emission is a fiber transfer — a potential suspension
    /// point. Polymorphic signals may also suspend (depends on the
    /// argument's signal at the call site).
    pub const fn may_suspend(&self) -> bool {
        !self.bits.is_empty() || self.propagates != 0
    }

    /// Can this function yield (cooperative suspension)?
    pub const fn may_yield(&self) -> bool {
        self.bits.intersects(SIG_YIELD)
    }

    /// Can this function PARK — suspend its frame into a continuation only a
    /// resume revives? The static face of [`dispatch::is_suspending`], asked
    /// of an inferred signal whose bits are a UNION of possible raises: any
    /// bit outside the two unwinding exits (`:error`, `:halt`) can park, so a
    /// compound like `:io`+`:error` answers true where the runtime predicate
    /// classifies one concrete raise at a time. `SIG_FFI` marks a foreign
    /// call, not a raise, and never parks. A polymorphic signal may park
    /// through its argument, so it answers true.
    pub const fn may_park(&self) -> bool {
        let non_park = SIG_ERROR.union(SIG_HALT).union(SIG_FFI);
        self.propagates != 0 || !self.bits.subtract(non_park).is_empty()
    }

    /// Can this function error?
    pub const fn may_error(&self) -> bool {
        self.bits.intersects(SIG_ERROR)
    }

    /// Can this function halt the VM?
    pub const fn may_halt(&self) -> bool {
        self.bits.intersects(SIG_HALT)
    }

    /// Does this function call foreign code?
    pub const fn may_ffi(&self) -> bool {
        self.bits.intersects(SIG_FFI)
    }

    /// Can this function perform I/O?
    pub const fn may_io(&self) -> bool {
        self.bits.intersects(SIG_IO)
    }

    /// Does this function's signal depend on its arguments?
    pub const fn is_polymorphic(&self) -> bool {
        self.propagates != 0
    }

    /// Get the set of parameter indices this signal propagates.
    pub fn propagated_params(&self) -> impl Iterator<Item = usize> {
        let mask = self.propagates;
        (0..32).filter(move |i| mask & (1 << i) != 0)
    }
}

// ── Constants ───────────────────────────────────────────────────────

impl Signal {
    pub const SILENT: Signal = Signal::silent();
    pub const YIELDS: Signal = Signal::yields();
}

impl fmt::Display for Signal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.propagates != 0 {
            let indices: Vec<_> = self.propagated_params().map(|i| i.to_string()).collect();
            write!(f, "polymorphic({})", indices.join(","))?;
        } else if self.bits.intersects(SIG_YIELD) {
            write!(f, "yields")?;
        } else if self.bits.intersects(SIG_IO) {
            // An async primitive raises `:io` and no longer claims `:yield`, so
            // without this arm every port and socket signal would print as
            // "silent" — the one word it is not.
            write!(f, "io")?;
        } else {
            write!(f, "silent")?;
        }

        // Append capability flags
        let mut flags = Vec::new();
        if self.bits.intersects(SIG_ERROR) {
            flags.push("errors");
        }
        if self.bits.intersects(SIG_HALT) {
            flags.push("halts");
        }
        if self.bits.intersects(SIG_FFI) {
            flags.push("ffi");
        }
        if self.bits.intersects(SIG_DEBUG) {
            flags.push("debug");
        }
        if !flags.is_empty() {
            write!(f, "+{}", flags.join("+"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
