//! Unit tests (`super` is the parent impl module).

use super::*;

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
    // Region/free diagnostics: functional today but checked via the
    // string `has_trace` (cold free paths in fiberheap/freelog.rs),
    // so they deliberately carry no `trace_bits` entry.
    const STRING_TRACED: &[&str] = &["free", "guardfree", "freebt"];
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
