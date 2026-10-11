// audited: 2026-10-06
//! What a call reports back to emitted WASM code.
//!
//! docs/impl/wasm.md

use crate::value::fiber::SignalBits;

/// The four words `rt_call` returns: a value, the signal it raised, and whether
/// the caller must park.
///
/// Emitted code reads `suspended` and never tests a bit of `signal`: which
/// signals park is `signals::dispatch::parks`'s rule, and [`Self::signalled`]
/// applies it. A callee can park carrying `:io` or `:error` (`suspended` true),
/// halt carrying a signal (`suspended` false), or return a plain value (both
/// clear).
///
/// This exists as a struct rather than a tuple because the two words are both
/// `i64` on the wire and mean opposite things: returning one where the other
/// belongs type-checks, compiles, and produces a silent wrong answer, the kind
/// `wasm::tests::tier` checks for. Build one through a
/// constructor and the pair cannot be crossed.
#[derive(Clone, Copy, Debug)]
pub(in crate::wasm) struct CallOutcome {
    pub tag: i64,
    pub payload: i64,
    pub signal: SignalBits,
    pub suspended: bool,
}

impl CallOutcome {
    /// A normal return: a value, no signal, no suspension.
    pub(in crate::wasm) fn value(tag: i64, payload: i64) -> Self {
        CallOutcome {
            tag,
            payload,
            signal: SignalBits::EMPTY,
            suspended: false,
        }
    }

    /// A callee that raised `signal`, classified by the shared rule.
    ///
    /// The only constructor that decides whether to park, and it defers to
    /// `signals::dispatch::parks` so the WASM tier and the interpreter cannot
    /// disagree about which signals park.
    pub(in crate::wasm) fn signalled(tag: i64, payload: i64, signal: SignalBits) -> Self {
        CallOutcome {
            tag,
            payload,
            signal,
            suspended: crate::signals::dispatch::parks(signal),
        }
    }

    /// A callee that parked carrying `signal`, where the caller already knows it
    /// parked and holds the signal itself.
    pub(in crate::wasm) fn parked(tag: i64, payload: i64, signal: SignalBits) -> Self {
        CallOutcome {
            tag,
            payload,
            signal,
            suspended: true,
        }
    }

    /// An error carrying `payload` as its value. It parks the caller, as
    /// every error does.
    pub(in crate::wasm) fn error(tag: i64, payload: i64) -> Self {
        Self::signalled(tag, payload, crate::value::fiber::SIG_ERROR)
    }

    /// The tuple the wasm import returns: `(tag, payload, signal, suspended)`.
    pub(in crate::wasm) fn to_wasm(self) -> (i64, i64, i64, i64) {
        (
            self.tag,
            self.payload,
            self.signal.raw() as i64,
            self.suspended as i64,
        )
    }
}
