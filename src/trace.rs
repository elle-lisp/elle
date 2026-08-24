//! Trace macro for runtime-gated debug output.
//!
//! Gates on the VM's `runtime_config.has_trace_bit` — one relaxed atomic load
//! of this instance's shared trace cell, no HashSet lookup.
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

/// True when `--trace=boot` is active. Boot marks fire before any VM
/// trace cell exists, so they gate on the static CLI config, like the
/// other string-traced keywords (`free`, `guardfree`).
pub(crate) fn boot() -> bool {
    crate::config::get().has_trace("boot")
}

/// True when `--trace=compile` is active. Compile phases run on the
/// compiler's own thread against the static CLI config — the same
/// gating the `[trace:regions]` dump in `compile_file_inner` uses.
pub(crate) fn compile() -> bool {
    crate::config::get().trace_bits() & crate::config::trace_bits::COMPILE != 0
}

/// Print one phase-timing mark: `[trace:SUBSYSTEM] LABEL 12.3ms`.
pub(crate) fn phase(enabled: bool, subsystem: &str, label: &str, start: std::time::Instant) {
    if enabled {
        eprintln!(
            "[trace:{}] {} {:.1}ms",
            subsystem,
            label,
            start.elapsed().as_secs_f64() * 1000.0
        );
    }
}
