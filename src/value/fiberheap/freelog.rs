//! Diagnostic free log: records the page address ranges freed by each
//! `free_runtime_region_pages`, with whether the free was a *direct* decref (the region's
//! own `decref_point`) or a *cascade* decref (a referrer region's `free_runtime_region_pages`
//! decref'd it). When `arena::deref` hits a tag/object mismatch (the
//! use-after-free signature), it consults this log to attribute the
//! stale payload address to the region (and free kind) that reclaimed it.
//!
//! This split tells two failure modes apart: a direct free means a liveness
//! bug (decref_point fires while the value is still live); a cascade free means
//! a missing cross-region incref on the referrer. The log is only populated
//! when `--trace=free` is set, so it is zero-cost otherwise.

use std::cell::{Cell, RefCell};

/// One `free_runtime_region_pages` event: the page ranges reclaimed and how it was triggered.
struct FreeRecord {
    region: u32,
    kind: String,
    seq: u64,
    ranges: Vec<(usize, usize)>,
    /// Trimmed backtrace of the call site that triggered the free.
    site: String,
}

thread_local! {
    static FREE_LOG: RefCell<Vec<FreeRecord>> = const { RefCell::new(Vec::new()) };
    static FREE_SEQ: RefCell<u64> = const { RefCell::new(0) };
    /// The reason for the *next* free, set by the bytecode/runtime call
    /// site immediately before it triggers a decref. Read (and reset) by
    /// `record_free`. Kept cheap so it doesn't perturb timing the way a
    /// symbolised backtrace did — important because this UAF only manifests
    /// when the recycled slot's tag mismatches, so any extra work at free
    /// time can mask it.
    static FREE_REASON: RefCell<String> = const { RefCell::new(String::new()) };
}

/// True when the `free` or `guardfree` trace flag is enabled.
pub(crate) fn enabled() -> bool {
    let c = crate::config::get();
    c.has_trace("free") || c.has_trace("guardfree")
}

thread_local! {
    /// Whether page-guarding (`--trace=guardfree`) is active. Off during
    /// stdlib init (which has its own benign init-time frees) and armed
    /// once user code starts, so the first fault is a real user-program UAF.
    static GUARD_ARMED: Cell<bool> = const { Cell::new(false) };
    /// The region whose contents the free-time cross-ref scan is currently
    /// walking (0 = not scanning). Set per member by `find_region_cross_refs`
    /// and cleared when it returns. The SIGSEGV attribution names the region
    /// that FREED the faulting page; this names the region still HOLDING the
    /// dangling edge into it (the over-freed target's referrer) when the fault
    /// lands inside the scan's `region_of_page_ptr`. A single Cell write per
    /// member — no allocation, so it does not perturb free-time timing.
    static SCAN_MEMBER: Cell<u32> = const { Cell::new(0) };
}

/// Mark the region whose contents the free-time cross-ref scan is walking (or 0
/// to clear). See `SCAN_MEMBER`.
pub(crate) fn set_scan_member(region: u32) {
    SCAN_MEMBER.with(|m| m.set(region));
}

/// Arm guardfree page-protection (called once stdlib init completes).
pub fn arm_guard() {
    GUARD_ARMED.with(|g| g.set(true));
    if crate::config::get().has_trace("guardfree") {
        install_segv_handler();
    }
}

extern "C" fn segv_handler(_sig: libc::c_int, info: *mut libc::siginfo_t, _ctx: *mut libc::c_void) {
    // Signal-handler context: best-effort diagnostic only. The fault
    // address is the use-after-free site; attribute it to the freeing
    // region via the (thread-local) free-log, then restore the default
    // handler and re-raise so a core/backtrace is still produced.
    let addr = unsafe { (*info).si_addr() as usize };
    let msg = freed_site(addr).unwrap_or_else(|| {
        format!("addr 0x{addr:x} faulted but is not in any recorded freed range")
    });
    let ctx = context();
    let held = SCAN_MEMBER.with(|m| m.get());
    let held_line = if held != 0 {
        format!("\n    held by: region {held} (its contents were being cross-ref scanned at its own free)")
    } else {
        String::new()
    };
    let s = format!(
        "\n[guardfree] SIGSEGV — use-after-free at 0x{addr:x}\n    {msg}{held_line}\n    context: {ctx}\n"
    );
    unsafe {
        libc::write(2, s.as_ptr() as *const libc::c_void, s.len());
        libc::signal(libc::SIGSEGV, libc::SIG_DFL);
        libc::raise(libc::SIGSEGV);
    }
}

fn install_segv_handler() {
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = segv_handler as *const () as usize;
        sa.sa_flags = libc::SA_SIGINFO;
        libc::sigemptyset(&mut sa.sa_mask);
        libc::sigaction(libc::SIGSEGV, &sa, std::ptr::null_mut());
    }
}

/// Whether freed pages should be guarded (mprotected) on release.
pub(crate) fn guard_armed() -> bool {
    crate::config::get().has_trace("guardfree") && GUARD_ARMED.with(|g| g.get())
}

/// Set the reason attributed to the next free (the call site about to
/// trigger a decref).
pub(crate) fn set_reason(reason: &'static str) {
    FREE_REASON.with(|r| *r.borrow_mut() = reason.to_string());
}

/// Set an owned reason (e.g. a formatted source span) for the next free.
pub(crate) fn set_reason_owned(reason: String) {
    FREE_REASON.with(|r| *r.borrow_mut() = reason);
}

thread_local! {
    /// Breadcrumb describing the macro expansion currently running, used
    /// to attribute a compile-time UAF to a specific stdlib macro call.
    static CONTEXT: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Record the macro/expansion context (name + source span).
pub(crate) fn set_context(ctx: String) {
    CONTEXT.with(|c| *c.borrow_mut() = ctx);
}

/// Current expansion context breadcrumb.
pub(crate) fn context() -> String {
    CONTEXT.with(|c| c.borrow().clone())
}

/// Record a `free_runtime_region_pages`: `region` freed `ranges` (page base..end addresses)
/// via `kind` ("direct" or "cascade(N)").
pub(crate) fn record_free(region: u32, kind: String, ranges: Vec<(usize, usize)>) {
    let seq = FREE_SEQ.with(|s| {
        let mut s = s.borrow_mut();
        *s += 1;
        *s
    });
    let site = FREE_REASON.with(|r| {
        let s = r.borrow().clone();
        r.borrow_mut().clear();
        if s.is_empty() {
            "unknown".to_string()
        } else {
            s
        }
    });
    FREE_LOG.with(|log| {
        log.borrow_mut().push(FreeRecord {
            region,
            kind,
            seq,
            ranges,
            site,
        })
    });
}

/// Attribution string for `addr` **only if** it falls inside a recorded
/// freed page range — `None` otherwise. Unlike `describe`, this never reads the
/// page memory, so it is safe to call on a guarded (mprotected) page: it answers
/// "was this address freed?" purely from the log. Used by use-site checks (e.g.
/// `UpdateCapture`) to turn a latent UAF into an immediate, attributed panic at
/// the consuming instruction.
pub(crate) fn freed_site(addr: usize) -> Option<String> {
    FREE_LOG.with(|log| {
        let log = log.borrow();
        let total = log.len();
        for (idx, rec) in log.iter().enumerate().rev() {
            if rec.ranges.iter().any(|&(b, e)| addr >= b && addr < e) {
                let frees_after = total - 1 - idx;
                return Some(format!(
                    "addr 0x{addr:x} was freed by region {} via {} \
                     (free #{} of {}, {} later frees)\n    free site: {}",
                    rec.region, rec.kind, rec.seq, total, frees_after, rec.site
                ));
            }
        }
        None
    })
}

/// Describe which freed region (if any) reclaimed the page containing
/// `addr`. Returns a human-readable attribution string, or `None` when the
/// log is empty or the address is not in any recorded freed range. Sole caller
/// is the `deref` tag/object-mismatch panic in `arena.rs`, itself
/// `#[cfg(debug_assertions)]`, so this is debug-only (absent — not dead — in
/// release).
#[cfg(debug_assertions)]
pub(crate) fn describe(addr: usize) -> Option<String> {
    FREE_LOG.with(|log| {
        let log = log.borrow();
        if log.is_empty() {
            return None;
        }
        let total = log.len();
        // List EVERY free that reclaimed a page containing `addr`, oldest
        // first — the first is the original (premature) free; the rest are
        // pagepool recycling churn after the address went stale.
        let matches: Vec<&FreeRecord> = log
            .iter()
            .filter(|rec| rec.ranges.iter().any(|&(b, e)| addr >= b && addr < e))
            .collect();
        if !matches.is_empty() {
            let mut s = format!(
                "addr 0x{addr:x} freed {} time(s) across {} total frees (oldest first):",
                matches.len(),
                total
            );
            for rec in &matches {
                s.push_str(&format!(
                    "\n      free #{} region {} via {} — {}",
                    rec.seq, rec.region, rec.kind, rec.site
                ));
            }
            return Some(s);
        }
        Some(format!(
            "addr 0x{addr:x} not in any of {} recorded freed page ranges",
            total
        ))
    })
}
