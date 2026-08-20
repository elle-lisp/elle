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

/// A window of 32-bit words around `addr` — the instructions a sampled PC
/// is parked on plus their neighborhood, read beside the photograph the
/// address came from (docs/impl/jit.md § "The code-address registry"). The
/// window runs from 16 bytes before `addr` (clamped to the nearest
/// registered entry) to 48 bytes after, sized so a PC parked just before an
/// AArch64 `LoadExtName` sequence still shows the 8-byte call-target
/// literal that follows its branch. Rendered four words per
/// `0x<addr>: 0x<w0> 0x<w1> 0x<w2> 0x<w3>` line; a line whose page is gone
/// renders `(unmapped)`. `None` when the address precedes every entry, sits
/// more than `PEEK_SPAN` past its nearest one, or its own 16 bytes are gone.
pub(crate) fn peek(addr: usize) -> Option<String> {
    let entries = snapshot();
    let base = entries.iter().rev().map(|(a, _)| *a).find(|a| *a <= addr)?;
    if addr - base >= PEEK_SPAN || !resident(addr, 16) {
        return None;
    }
    let addr = addr & !3; // word-align a mid-instruction sample
    let start = addr.saturating_sub(16).max(base);
    let end = addr + 48;
    use std::fmt::Write;
    let mut out = String::new();
    let mut line = start;
    while line < end {
        if !out.is_empty() {
            out.push('\n');
        }
        let _ = write!(out, "{:#x}:", line);
        if resident(line, 16) {
            for i in 0..4 {
                let w = unsafe { ((line + i * 4) as *const u32).read_volatile() };
                let _ = write!(out, " {:#010x}", w);
            }
        } else {
            out.push_str(" (unmapped)");
        }
        line += 16;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Distinct, recognizable word values: `0xa5a50000 + index`.
    static WORDS: [u32; 32] = {
        let mut w = [0u32; 32];
        let mut i = 0;
        while i < 32 {
            w[i] = 0xa5a5_0000 + i as u32;
            i += 1;
        }
        w
    };

    #[test]
    fn peek_renders_a_window_around_the_address() {
        let base = WORDS.as_ptr() as usize;
        record(base, "peek-window-probe");
        // Word index 6: the window spans [addr-16, addr+48) = indices 2..18.
        let addr = base + 24;
        let s = peek(addr).expect("registered resident address answers");
        // Context before the address: index 2 (addr-16).
        assert!(
            s.contains("0xa5a50002"),
            "window reaches back 16 bytes: {s}"
        );
        // The queried word itself.
        assert!(s.contains("0xa5a50006"), "window covers the address: {s}");
        // A LoadExtName literal parked 16 bytes past a sampled PC: index 10.
        assert!(
            s.contains("0xa5a5000a"),
            "window reaches 16 bytes past: {s}"
        );
        // The window's last word: index 17 (addr+44).
        assert!(
            s.contains("0xa5a50011"),
            "window reaches 48 bytes past: {s}"
        );
        // Each line names its own address.
        for line in s.lines() {
            assert!(line.starts_with("0x"), "line carries its address: {line}");
        }
        assert_eq!(s.lines().count(), 4, "four lines of four words: {s}");
        // Below every entry: nothing precedes a null-page address.
        assert!(peek(0x10).is_none());
        // Past the span of its nearest entry.
        assert!(peek(base + PEEK_SPAN).is_none());
    }

    #[test]
    fn peek_clamps_the_window_at_the_registered_entry() {
        let base = WORDS.as_ptr() as usize;
        record(base, "peek-clamp-probe");
        // At the entry itself there is nothing before it to show: the
        // first rendered line is the entry's own address.
        let s = peek(base).expect("entry address answers");
        let first = s.lines().next().expect("at least one line");
        assert!(
            first.starts_with(&format!("{base:#x}:")),
            "window must start at the entry, not below it: {s}"
        );
        assert!(s.contains("0xa5a50000"), "entry word present: {s}");
    }
}
