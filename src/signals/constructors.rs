// audited: 2026-10-06
//! The named `Signal` constructors, and the combinators that build one signal from others.
//!
//! docs/signals/protocol.md

use super::*;

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

    /// Reads a file and runs foreign code from it, and may error
    /// (SIG_FS | SIG_FFI | SIG_ERROR).
    ///
    /// The signal of `import/load-plugin`, which reads a shared library and runs
    /// its init. Denying either capability blocks the load.
    pub const fn fs_ffi_errors() -> Self {
        Signal {
            bits: SIG_FS.union(SIG_FFI).union(SIG_ERROR),
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
