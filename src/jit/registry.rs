//! The process-global JIT code-address registry.
//!
//! Native samplers cannot name JIT frames: the code lives in anonymous
//! Cranelift mappings, so a thread photograph shows `??? (in <unknown
//! binary>)` exactly where the answer is. Every successful compile records
//! its entry address and function name here, and `(vm/query "jit/map" nil)`
//! renders the table for the reader holding such a photograph. See
//! docs/impl/jit.md § "The code-address registry".
//!
//! One table for the whole process, fed from every VM's compile paths (the
//! background `elle-jit` workers and the synchronous tier probes alike).
//! Entries are never removed: a stack captured at any time may reference
//! code whose `JitCode` has since been dropped, and a dangling name is
//! still the right label for the frame that ran it.

use std::sync::Mutex;

static ENTRIES: Mutex<Vec<(usize, String)>> = Mutex::new(Vec::new());

/// Record one compiled function's entry address. Called once per successful
/// compile; duplicate addresses (a page reused after a module was dropped)
/// simply shadow the older entry in `render`, which sorts by address and
/// keeps the later insertion.
pub(crate) fn record(entry: usize, name: &str) {
    let mut entries = ENTRIES.lock().expect("jit registry lock poisoned");
    entries.push((entry, name.to_string()));
}

/// A copy of the table, sorted by address.
pub(crate) fn snapshot() -> Vec<(usize, String)> {
    let mut entries = ENTRIES.lock().expect("jit registry lock poisoned").clone();
    entries.sort_by_key(|(addr, _)| *addr);
    entries
}

/// The table as text: one `0x<addr> <name>` line per entry, sorted by
/// address. A sampled JIT frame resolves to the nearest preceding entry.
pub(crate) fn render() -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for (addr, name) in snapshot() {
        let _ = writeln!(out, "{:#x} {}", addr, name);
    }
    out
}
