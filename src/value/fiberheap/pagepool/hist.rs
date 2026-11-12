// audited: 2026-11-12
//! The page-claim size histogram `--dump=stats` prints at exit, for page-size analysis.
//!
//! docs/impl/region/diagnostics.md
//!
//! Under `--dump=stats`, every `claim` records its (power-of-two) size into a
//! global per-size-class histogram, printed at process exit alongside the other
//! `[stats]` lines. It measures how often geometric growth escalates past
//! `base_page()` across a run — the precondition for the large-page
//! region-attribution cost (a large page is the only place `region_of_ptr`'s
//! sub-alignment search can be fooled). Off by default and zero-cost then: the
//! per-claim path is a single config-flag read, so production pays nothing.

use super::{base_page, class_size_of, size_class, NUM_CLASSES};
use std::sync::atomic::{AtomicU64, Ordering};

static PAGE_CLAIM_COUNT: [AtomicU64; NUM_CLASSES + 1] =
    [const { AtomicU64::new(0) }; NUM_CLASSES + 1];
static PAGE_CLAIM_BYTES: [AtomicU64; NUM_CLASSES + 1] =
    [const { AtomicU64::new(0) }; NUM_CLASSES + 1];

/// Whether `--dump=stats` is active (the histogram's gate). Off ⇒ `record_claim` and
/// `dump_page_hist` are no-ops, so the histogram costs nothing in normal runs.
/// Read live from the global config like `has_trace`. A page claimed before
/// `config::init` reads the defaults and does not freeze them
/// (`config::get`), so this needs no ordering rule.
fn page_hist_enabled() -> bool {
    crate::config::get().stats
}

/// Count one claim of `size` bytes in its size class.
pub(super) fn record_claim(size: usize) {
    if !page_hist_enabled() {
        return;
    }
    // Register the at-exit dump on first recorded claim, so it fires even when
    // the test runner ends via `os/exit` (which runs C atexit handlers but not
    // Rust destructors, so the main-thread `--dump=stats` block is bypassed).
    static REGISTER: std::sync::Once = std::sync::Once::new();
    REGISTER.call_once(|| unsafe {
        extern "C" fn at_exit() {
            dump_page_hist();
        }
        libc::atexit(at_exit);
    });
    let bucket = size_class(size).min(NUM_CLASSES);
    PAGE_CLAIM_COUNT[bucket].fetch_add(1, Ordering::Relaxed);
    PAGE_CLAIM_BYTES[bucket].fetch_add(size as u64, Ordering::Relaxed);
}

/// Print the page-claim histogram to stderr under `--dump=stats`: one `[stats]
/// page-claim size=<bytes> claims=<n> bytes=<n>` line per non-empty size class
/// (`size=0` = the oversized one-off bucket above the size classes). The fields
/// are stable so a batched corpus run can sum them across processes.
pub(crate) fn dump_page_hist() {
    if !page_hist_enabled() {
        return;
    }
    for c in 0..=NUM_CLASSES {
        let n = PAGE_CLAIM_COUNT[c].load(Ordering::Relaxed);
        if n == 0 {
            continue;
        }
        let bytes = PAGE_CLAIM_BYTES[c].load(Ordering::Relaxed);
        let size = if c < NUM_CLASSES {
            class_size_of(base_page(), c)
        } else {
            0
        };
        eprintln!("[stats] page-claim size={size} claims={n} bytes={bytes}");
    }
}
