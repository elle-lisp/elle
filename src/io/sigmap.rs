//! audited: 2026-09-17
//! Shared keyword↔signum mapping for POSIX signals.
//!
//! Used by `src/primitives/subprocess.rs` (for `subprocess/kill`) and
//! `src/primitives/posix.rs` (for `os/sig-send`, `os/sig-raise`, `os/sig-watch`,
//! and friends). Only the 16 standard signals enumerated below are
//! recognised — realtime signals (SIGRTMIN..SIGRTMAX) are intentionally
//! not exposed in v1.
//!
//! Integer signums are accepted by the callers only if they round-trip
//! through `signum_to_keyword`; this rejects arbitrary integers and
//! tightens a long-standing footgun in `subprocess/kill`.

/// All recognised signals: keyword name (without the leading `:`) and
/// the libc constant.
const SIGNALS: &[(&str, libc::c_int)] = &[
    ("sigterm", libc::SIGTERM),
    ("sigkill", libc::SIGKILL),
    ("sighup", libc::SIGHUP),
    ("sigint", libc::SIGINT),
    ("sigquit", libc::SIGQUIT),
    ("sigpipe", libc::SIGPIPE),
    ("sigalrm", libc::SIGALRM),
    ("sigusr1", libc::SIGUSR1),
    ("sigusr2", libc::SIGUSR2),
    ("sigchld", libc::SIGCHLD),
    ("sigcont", libc::SIGCONT),
    ("sigstop", libc::SIGSTOP),
    ("sigtstp", libc::SIGTSTP),
    ("sigttin", libc::SIGTTIN),
    ("sigttou", libc::SIGTTOU),
    ("sigwinch", libc::SIGWINCH),
];

/// The signals a process *dies* on that `SIGNALS` deliberately omits. The CPU
/// raises them at an instruction, or `abort(3)` does; a program does not send
/// them, so `subprocess/kill` and `os/sig-send` go on refusing them.
///
/// Naming one is a different question from sending one. A child's terminating
/// status has to read as something, and `signal 11` is not what the reader of a
/// test result needs. The numbers differ by platform — SIGBUS is 7 on Linux and
/// 10 on macOS — so only libc can answer, which is why this table is here and
/// not in the caller.
const FATAL_SIGNALS: &[(&str, libc::c_int)] = &[
    ("sigsegv", libc::SIGSEGV),
    ("sigabrt", libc::SIGABRT),
    ("sigbus", libc::SIGBUS),
    ("sigill", libc::SIGILL),
    ("sigfpe", libc::SIGFPE),
    ("sigtrap", libc::SIGTRAP),
    ("sigsys", libc::SIGSYS),
];

/// The canonical keyword name of any signal this build knows — one a program
/// may send, or one it may die on. `None` for anything else.
pub fn signum_name(signum: libc::c_int) -> Option<&'static str> {
    signum_to_keyword(signum).or_else(|| {
        FATAL_SIGNALS
            .iter()
            .find_map(|(k, v)| if *v == signum { Some(*k) } else { None })
    })
}

/// Map a keyword name (without the colon, e.g. "sigterm") to its libc constant.
pub fn keyword_to_signum(name: &str) -> Option<libc::c_int> {
    SIGNALS
        .iter()
        .find_map(|(k, v)| if *k == name { Some(*v) } else { None })
}

/// Inverse: map a libc signum back to its canonical keyword name.
/// Returns `None` for any unrecognised signum (the basis of the integer
/// validation contract).
pub fn signum_to_keyword(signum: libc::c_int) -> Option<&'static str> {
    SIGNALS
        .iter()
        .find_map(|(k, v)| if *v == signum { Some(*k) } else { None })
}

/// Human-readable list of supported keyword names, comma-separated and
/// colon-prefixed. Used in error messages.
pub fn supported_list_str() -> String {
    SIGNALS
        .iter()
        .map(|(k, _)| format!(":{k}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Resolve an Elle Value to a libc signum.
///
/// Accepts:
/// - a keyword (e.g. `:sigterm`),
/// - a named integer (any integer that round-trips through
///   `signum_to_keyword`).
///
/// Unknown keywords and unnamed integers return an error string.
/// `context` is the primitive name used in error messages. `memo` is the
/// calling instance's display memo, which is what turns the rejected
/// keyword's hash back into the spelling the caller wrote.
pub fn resolve(
    val: &crate::value::Value,
    context: &str,
    memo: Option<&crate::symbol::SymbolTable>,
) -> Result<libc::c_int, ResolveError> {
    if let Some(n) = val.as_int() {
        let n_i32 = n as libc::c_int;
        return match signum_to_keyword(n_i32) {
            Some(_) => Ok(n_i32),
            None => Err(ResolveError::UnknownSignum(n)),
        };
    }
    if let Some(hash) = val.keyword_hash() {
        // Match by hash — the recognised set is fixed, so no spelling is
        // needed to resolve. A miss recovers a spelling for the error message
        // through the memo, then the static vocabulary. The rejected keyword
        // is the caller's own, so it is the memo that holds it; without one
        // the message names a hash the caller has to decode.
        for (k, v) in SIGNALS {
            if crate::value::keyword::keyword_hash(k) == hash {
                return Ok(*v);
            }
        }
        let spelling = crate::value::keyword::resolve_keyword_name(memo, hash)
            .map(|n| format!(":{}", n))
            .unwrap_or_else(|| format!("#<keyword:{:#x}>", hash));
        return Err(ResolveError::UnknownKeyword(spelling));
    }
    let _ = context;
    Err(ResolveError::WrongType(val.type_name()))
}

/// Failure modes from `resolve`. Carries enough information for callers
/// to construct their own error Value with the right kind tag.
#[derive(Debug)]
pub enum ResolveError {
    /// Integer did not round-trip to a named signal.
    UnknownSignum(i64),
    /// Keyword is not in the recognised set, already rendered as the caller
    /// would read it — `:name` when a spelling was found, the unreadable
    /// `#<keyword:hash>` when none was.
    UnknownKeyword(String),
    /// Argument is neither integer nor keyword.
    WrongType(&'static str),
}

impl ResolveError {
    /// Return (error-kind, message) for use in `error_val(kind, msg)`.
    pub fn parts(&self, context: &str) -> (&'static str, String) {
        match self {
            ResolveError::UnknownSignum(n) => (
                "argument-error",
                format!(
                    "{}: signum {} is not a named signal; expected one of {} or the equivalent integer",
                    context,
                    n,
                    supported_list_str(),
                ),
            ),
            ResolveError::UnknownKeyword(shown) => (
                "argument-error",
                format!(
                    "{}: unknown signal keyword {}; expected one of {}",
                    context,
                    shown,
                    supported_list_str(),
                ),
            ),
            ResolveError::WrongType(t) => (
                "type-error",
                format!("{}: signal must be integer or keyword, got {}", context, t),
            ),
        }
    }
}

#[cfg(test)]
mod tests;
