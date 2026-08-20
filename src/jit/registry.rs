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

/// How far past a registered entry an address may sit and still be peeked.
/// Entries record starts, not sizes; a compiled function fits well inside
/// this, and the residency check below is what actually guards the read.
const PEEK_SPAN: usize = 1 << 20;

/// True when every page of `[addr, addr+len)` is mapped in this process.
/// `mincore` answers ENOMEM for an unmapped page, which is what a peek of a
/// dropped module's address would otherwise fault on.
fn resident(addr: usize, len: usize) -> bool {
    let page = (unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).max(4096) as usize;
    let start = addr & !(page - 1);
    let span = addr + len - start;
    let mut vec = vec![0u8; span.div_ceil(page)];
    unsafe { libc::mincore(start as *mut libc::c_void, span, vec.as_mut_ptr() as *mut _) == 0 }
}

/// The four 32-bit words at `addr`, rendered `0x<w0> 0x<w1> 0x<w2> 0x<w3>`
/// in address order — the instructions a sampled PC is parked on, read
/// beside the photograph the address came from (docs/impl/jit.md § "The
/// code-address registry"). `None` when the address precedes every entry,
/// sits more than `PEEK_SPAN` past its nearest one, or its pages are gone.
pub(crate) fn peek(addr: usize) -> Option<String> {
    let entries = snapshot();
    let base = entries.iter().rev().map(|(a, _)| *a).find(|a| *a <= addr)?;
    if addr - base >= PEEK_SPAN || !resident(addr, 16) {
        return None;
    }
    let words: Vec<String> = (0..4)
        .map(|i| {
            let w = unsafe { ((addr + i * 4) as *const u32).read_volatile() };
            format!("{:#010x}", w)
        })
        .collect();
    Some(words.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peek_reads_a_registered_span_and_refuses_the_rest() {
        static WORDS: [u32; 4] = [0x1400_0000, 1, 2, 3];
        let addr = WORDS.as_ptr() as usize;
        record(addr, "peek-span-probe");
        let s = peek(addr).expect("registered resident address answers");
        assert!(
            s.starts_with("0x14000000"),
            "words render in address order: {s}"
        );
        assert_eq!(s.split(' ').count(), 4);
        // Below every entry: nothing precedes a null-page address.
        assert!(peek(0x10).is_none());
        // Past the span of its nearest entry.
        assert!(peek(addr + PEEK_SPAN).is_none());
    }
}
