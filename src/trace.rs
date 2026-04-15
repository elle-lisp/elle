//! Trace macro for runtime-gated debug output.
//!
//! Uses the VM's `runtime_config.trace_bits` bitfield for fast hot-path
//! checks. No HashSet lookup — just a bitwise AND.
//!
//! Format: `[trace:SUBSYSTEM] message` for easy grep filtering.

/// Emit a trace message to stderr if the given trace bit is active.
///
/// The first argument is a reference to the VM (or anything with a
/// `runtime_config` field). The second is a trace bit constant from
/// `crate::config::trace_bits`. Remaining arguments are passed to
/// `eprintln!`.
///
/// Hot-path cost when tracing is off: one bitwise AND + branch.
#[macro_export]
macro_rules! etrace {
    ($vm:expr, $bit:expr, $subsystem:expr, $($arg:tt)*) => {
        if $vm.runtime_config.has_trace_bit($bit) {
            eprintln!(concat!("[trace:", $subsystem, "] {}"), format_args!($($arg)*));
        }
    };
}
