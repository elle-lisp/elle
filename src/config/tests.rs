//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::value::fiberheap::pagepool::base_page;

/// Trace state is per-instance: a `RuntimeConfig` reads and writes its own
/// [`TraceCell`], so a diagnostic toggle on one instance never reaches another.
/// This is the property the corpus runner relies on to keep a `--trace=`-heavy
/// file from bleeding into a parallel file's run — before the relocation the two
/// shared a process-global atomic, so any instance's `set_trace` flipped the bit
/// every off-VM reader saw.
#[test]
fn trace_bits_are_per_cell_not_global() {
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    let cell_a: TraceCell = Arc::new(AtomicU32::new(0));
    let cell_b: TraceCell = Arc::new(AtomicU32::new(0));
    let cfg = Config::default();
    let mut a = RuntimeConfig::from_static_config(&cfg, Arc::clone(&cell_a));
    let b = RuntimeConfig::from_static_config(&cfg, Arc::clone(&cell_b));

    // Enable :call on instance A only.
    a.set_trace(HashSet::from(["call".to_string()]));

    assert!(a.has_trace_bit(trace_bits::CALL), "A sees its own :call");
    assert!(
        !b.has_trace_bit(trace_bits::CALL),
        "B must NOT see A's :call — the cells are independent per-instance"
    );
    assert_eq!(
        cell_a.load(Ordering::Relaxed) & trace_bits::CALL,
        trace_bits::CALL
    );
    assert_eq!(
        cell_b.load(Ordering::Relaxed) & trace_bits::CALL,
        0,
        "B's authoritative cell is untouched by A's set_trace"
    );
}

/// A reader holding a *clone* of the same cell (the region pool's `PAGES` gate, a
/// channel's `WakeList`) observes the instance's live trace — one shared bitfield
/// updated in place by `set_trace`, so a runtime toggle reaches the off-VM readers
/// without a process-global.
#[test]
fn trace_cell_clone_observes_updates() {
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    let cell: TraceCell = Arc::new(AtomicU32::new(0));
    // Stands in for a RegionPool / WakeList clone of the heap's cell.
    let reader = Arc::clone(&cell);
    let mut cfg = RuntimeConfig::from_static_config(&Config::default(), Arc::clone(&cell));

    assert_eq!(reader.load(Ordering::Relaxed) & trace_bits::PAGES, 0);
    cfg.set_trace(HashSet::from(["pages".to_string()]));
    assert_eq!(
        reader.load(Ordering::Relaxed) & trace_bits::PAGES,
        trace_bits::PAGES,
        "an off-VM reader holding a clone of the cell sees the live update"
    );
}

/// Every defined trace bit must be reachable via `--trace=all`.
///
/// `--trace=all` expands to exactly `TRACE_KEYWORDS` (see `Config::parse`),
/// then each keyword is OR'd through `trace_bits::from_name`. If a real
/// keyword (one that maps to a non-zero bit) is missing from the array,
/// `--trace=all` silently skips that subsystem even though `--trace=<kw>`
/// works when named explicitly. This guards against re-introducing that
/// drift (e.g. `chan` and `anf`, which were each added with a bit +
/// `from_name` entry + `--help` line but originally forgotten here).
#[test]
fn trace_all_covers_every_defined_bit() {
    let from_all: u32 = TRACE_KEYWORDS
        .iter()
        .fold(0, |acc, kw| acc | trace_bits::from_name(kw));
    assert_eq!(
        from_all,
        trace_bits::ALL,
        "--trace=all does not cover every defined trace bit; \
             missing bits: {:#b}",
        trace_bits::ALL & !from_all
    );
}

/// Conversely, every keyword listed in `TRACE_KEYWORDS` must either map to
/// a real bit or be one of the documented bit-less keywords. Catches typos
/// in the array that would make `--trace=all` a silent no-op for that entry.
#[test]
fn trace_keywords_are_known() {
    // Future GPU backends — accepted without error, no bit yet.
    const FORWARD_COMPAT: &[&str] = &["spirv", "mlir", "gpu"];
    // Region/free diagnostics, boot-phase timing, the post-boot census, the
    // park trace, and the syncjit compile mode: functional today but checked
    // via the string `has_trace` (cold free paths in fiberheap/freelog.rs;
    // the phase marks and census in trace.rs; the park/resume seam; the
    // submit path in vm/jit_entry.rs), so they deliberately carry no
    // `trace_bits` entry.
    const STRING_TRACED: &[&str] = &[
        "free",
        "guardfree",
        "freebt",
        "scrub",
        "boot",
        "census",
        "park",
        "syncjit",
    ];
    for kw in TRACE_KEYWORDS {
        let recognized = trace_bits::from_name(kw) != 0
            || FORWARD_COMPAT.contains(kw)
            || STRING_TRACED.contains(kw);
        assert!(
            recognized,
            "TRACE_KEYWORDS entry {:?} maps to no trace bit and is not a \
                 documented forward-compat or string-traced keyword",
            kw
        );
    }
}

// ── `--region-page-size` (docs/impl/region/model.md § "The base page is the OS
// page") ──

fn parse_args(args: &[&str]) -> Result<Config, String> {
    let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    Config::parse(&owned).map(|(c, _)| c)
}

/// A region's first page is one OS page, so a program that sets nothing gets
/// the page the kernel charges for rather than a fraction of it.
#[test]
fn region_page_size_defaults_to_the_base_page() {
    assert_eq!(Config::default().region_page_size, base_page());
    assert_eq!(parse_args(&[]).unwrap().region_page_size, base_page());
}

/// The floor is the OS page, not a fixed 4096. A smaller page still costs a
/// whole OS page, and `MmapPage::new` would trim it to an address `munmap`
/// refuses.
#[test]
fn region_page_size_below_the_base_page_is_rejected() {
    let err = parse_args(&["--region-page-size=2048"]).unwrap_err();
    assert!(
        err.contains(&base_page().to_string()),
        "the rejection must name the floor it applied, got {err:?}",
    );
    assert!(parse_args(&["--region-page-size=6000"]).is_err());
    assert_eq!(
        parse_args(&[&format!("--region-page-size={}", base_page())])
            .unwrap()
            .region_page_size,
        base_page(),
    );
    assert_eq!(
        parse_args(&[&format!("--region-page-size={}", 4 * base_page())])
            .unwrap()
            .region_page_size,
        4 * base_page(),
    );
}
