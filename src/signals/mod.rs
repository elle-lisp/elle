//! Signal type for tracking which signals a function may emit.
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
//! of a call site based on its arguments. `SignalBits` (in `value/fiber.rs`) is
//! the **runtime** representation: a flat bitmask stored on closures and used by
//! the VM and JIT for dispatch. These are intentionally separate types serving
//! different phases. The `propagates` field has no runtime analogue. Do not
//! attempt to unify them.

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
//   Bit  6:     Abort — graceful fiber termination with error injection (VM-internal)
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
//
// "Capability bit" says only that the bit selects no I/O backend: `SIG_IO` is
// what routes a request to the scheduler, and `:exec` rides alongside it rather
// than replacing it. It does NOT mean the bit is inert in a fiber mask. A mask
// catches a signal on any shared bit, so `|:exec|` catches a subprocess request
// exactly as `|:io|` does — see `SignalBits::covers` and #895.
//   Bits 18-31: Runtime-reserved (future runtime signals)
//   Bits 32-63: User-defined signal types

pub const SIG_OK: SignalBits = SignalBits::EMPTY; // no bits set = normal return
pub const SIG_ERROR: SignalBits = SignalBits::new(1 << 0); // exception / panic
pub const SIG_YIELD: SignalBits = SignalBits::new(1 << 1); // cooperative suspension
pub const SIG_DEBUG: SignalBits = SignalBits::new(1 << 2); // breakpoint / trace
pub const SIG_RESUME: SignalBits = SignalBits::new(1 << 3); // fiber resumption (VM-internal)
pub const SIG_FFI: SignalBits = SignalBits::new(1 << 4); // calls foreign code
pub const SIG_PROPAGATE: SignalBits = SignalBits::new(1 << 5); // propagate caught signal (VM-internal)
pub const SIG_ABORT: SignalBits = SIG_ERROR.union(SIG_TERMINAL); // graceful fiber termination with error injection (VM-internal)
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

/// VM-internal signal bits: infrastructure signals that user code cannot
/// produce. These are emitted exclusively by the VM's own dispatch machinery.
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

// ── Constructors ────────────────────────────────────────────────────

impl Signal {
    /// No signals: does not signal, does not propagate.
    pub const fn silent() -> Self {
        Signal {
            bits: SignalBits::new(0),
            propagates: 0,
        }
    }

    /// May error (most primitives: arity/type errors).
    pub const fn errors() -> Self {
        Signal {
            bits: SIG_ERROR,
            propagates: 0,
        }
    }

    /// May yield (cooperative suspension).
    pub const fn yields() -> Self {
        Signal {
            bits: SIG_YIELD,
            propagates: 0,
        }
    }

    /// May yield and may error.
    pub const fn yields_errors() -> Self {
        Signal {
            bits: SIG_YIELD.union(SIG_ERROR),
            propagates: 0,
        }
    }

    /// May halt the VM (non-resumable termination with return value).
    pub const fn halts() -> Self {
        Signal {
            bits: SIG_HALT.union(SIG_ERROR),
            propagates: 0,
        }
    }

    /// Calls foreign code via FFI.
    pub const fn ffi() -> Self {
        Signal {
            bits: SIG_FFI,
            propagates: 0,
        }
    }

    /// Calls foreign code and may error (SIG_FFI | SIG_ERROR).
    /// Used for FFI primitives that validate arguments before calling C.
    pub const fn ffi_errors() -> Self {
        Signal {
            bits: SIG_FFI.union(SIG_ERROR),
            propagates: 0,
        }
    }

    /// Performs asynchronous I/O: raises a request and may error
    /// (SIG_IO | SIG_ERROR).
    ///
    /// The signal of every port, socket, and file primitive that reaches the
    /// scheduler. The request suspends the calling fiber until the backend
    /// completes it, but that is true of any signal and needs no bit of its own
    /// — see the `IO_ROUND_TRIP` constant.
    pub const fn io_yields_errors() -> Self {
        Signal {
            bits: IO_ROUND_TRIP,
            propagates: 0,
        }
    }

    /// Resolves a filesystem path and may error (SIG_FS | SIG_ERROR).
    ///
    /// The signal of every primitive whose implementation reaches the
    /// filesystem synchronously. `SIG_FS` is a capability bit only: these are
    /// `std::fs` calls that return their result directly, so nothing about
    /// dispatch or the event loop changes. Carrying `SIG_IO` instead would be
    /// wrong twice over — it claims a scheduler round trip that never happens,
    /// and it ties the disk to the bit that governs ports and sockets.
    pub const fn fs_errors() -> Self {
        Signal {
            bits: SIG_FS.union(SIG_ERROR),
            propagates: 0,
        }
    }

    /// Opens a filesystem path through the I/O scheduler
    /// (SIG_FS | SIG_IO | SIG_ERROR).
    ///
    /// Both capabilities apply and either denial blocks the call: `SIG_FS` for
    /// the path the primitive resolves, `SIG_IO` for the scheduler round trip
    /// that opens it. Without `SIG_FS`, a fiber denied only the filesystem
    /// could open a port on any path and read it — the same authority
    /// `file/read` grants.
    pub const fn fs_io_yields_errors() -> Self {
        Signal {
            bits: IO_ROUND_TRIP.union(SIG_FS),
            propagates: 0,
        }
    }

    /// Runs a subprocess: asynchronous I/O under the exec capability
    /// (SIG_EXEC | SIG_IO | SIG_ERROR).
    ///
    /// Both `SIG_EXEC` and `SIG_IO` are emitted, and they do different jobs.
    /// `SIG_IO` is the dispatch bit that routes the request through the I/O
    /// scheduler; `SIG_EXEC` is the capability bit a fiber mask tests to
    /// permit or deny spawning at all.
    pub const fn subprocess() -> Self {
        Signal {
            bits: IO_ROUND_TRIP.union(SIG_EXEC),
            propagates: 0,
        }
    }

    /// Asks the VM about the current fiber and may error
    /// (SIG_QUERY | SIG_ERROR).
    ///
    /// A query cannot read what it needs from its arguments — the answer is
    /// the running fiber's own state — so it returns `SIG_QUERY` and the VM
    /// answers it.
    pub const fn query_errors() -> Self {
        Signal {
            bits: SIG_QUERY.union(SIG_ERROR),
            propagates: 0,
        }
    }

    /// An arbitrary set of emitted bits, propagating no parameter.
    ///
    /// For the signals with no name of their own. Prefer a named constructor
    /// where one fits: the name says what the primitive does, where a bit set
    /// only says which bits it sets.
    pub const fn of(bits: SignalBits) -> Self {
        Signal {
            bits,
            propagates: 0,
        }
    }

    /// Sends a POSIX signal (capability-gated) and may error (SIG_OS_SIGNAL | SIG_ERROR).
    /// Used by os/sig-send and os/sig-raise.
    pub const fn os_signal_errors() -> Self {
        Signal {
            bits: SIG_OS_SIGNAL.union(SIG_ERROR),
            propagates: 0,
        }
    }

    /// Maximally conservative signal for a callee whose effects are unknown.
    /// Includes all user-facing signal bits (CAP_MASK): the callee could
    /// error, yield, do I/O, call foreign code, exec subprocesses, halt
    /// the VM, or trigger a debug breakpoint. Used when calling a value
    /// whose origin is opaque to static analysis (e.g., a local bound to
    /// a dynamic expression, or calling the result of an arbitrary
    /// expression).
    pub const fn unknown() -> Self {
        Signal {
            bits: CAP_MASK,
            propagates: 0,
        }
    }

    /// Polymorphic: signal depends on a single parameter (no error signal).
    pub const fn polymorphic(param: usize) -> Self {
        Signal {
            bits: SignalBits::new(0),
            propagates: 1 << param,
        }
    }

    /// Polymorphic: signal depends on a single parameter (may error).
    pub const fn polymorphic_errors(param: usize) -> Self {
        Signal {
            bits: SIG_ERROR,
            propagates: 1 << param,
        }
    }

    /// Combine two signals (used for sequencing).
    /// Signal bits are ORed. Propagation masks are ORed.
    pub const fn combine(self, other: Signal) -> Signal {
        Signal {
            bits: self.bits.union(other.bits),
            propagates: self.propagates | other.propagates,
        }
    }

    /// Combine multiple signals.
    pub fn combine_all(signals: impl IntoIterator<Item = Signal>) -> Signal {
        signals
            .into_iter()
            .fold(Signal::silent(), |a, b| a.combine(b))
    }

    /// Compute the compile-time signal after squelching the given mask.
    ///
    /// Mirrors `Closure::effective_signal()` at runtime: if the mask
    /// suppresses signals this function actually emits, those bits are
    /// cleared and SIG_ERROR is added (squelch converts to error).
    /// When the mask doesn't suppress anything, returns self unchanged.
    pub const fn squelch(self, mask: SignalBits) -> Signal {
        let actually_squelched = self.bits.intersection(mask);
        if actually_squelched.is_empty() {
            return self;
        }
        Signal {
            bits: self.bits.subtract(mask).union(SIG_ERROR),
            propagates: self.propagates,
        }
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
