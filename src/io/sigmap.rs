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
/// `context` is the primitive name used in error messages.
pub fn resolve(val: &crate::value::Value, context: &str) -> Result<libc::c_int, ResolveError> {
    if let Some(n) = val.as_int() {
        let n_i32 = n as libc::c_int;
        return match signum_to_keyword(n_i32) {
            Some(_) => Ok(n_i32),
            None => Err(ResolveError::UnknownSignum(n)),
        };
    }
    if let Some(name) = val.as_keyword_name() {
        return match keyword_to_signum(&name) {
            Some(s) => Ok(s),
            None => Err(ResolveError::UnknownKeyword(name.to_string())),
        };
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
    /// Keyword name is not in the recognised set.
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
            ResolveError::UnknownKeyword(name) => (
                "argument-error",
                format!(
                    "{}: unknown signal keyword :{}; expected one of {}",
                    context,
                    name,
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
