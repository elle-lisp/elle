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

/// `--region-ownership` no longer forces the JIT off. The adopt/group ops are
/// lowered on the JIT (`elle_jit_adopt_region`/`elle_jit_free_region_group`),
/// mirroring the interpreter handlers, so VM≡JIT parity holds and an explicit
/// `--jit` must survive the post-loop normalization — it was clobbered to `Off`
/// while those cuts were VM-only. MLIR/WASM still trail (forced off).
#[test]
fn region_ownership_keeps_explicit_jit_on() {
    let (cfg, _rest) =
        Config::parse(&["--region-ownership".to_string(), "--jit=eager".to_string()])
            .expect("parses");
    assert!(
        cfg.region_ownership,
        "--region-ownership sets the forest flag"
    );
    assert!(
        cfg.jit.enabled(),
        "an explicit --jit under --region-ownership must NOT be forced off — the \
         adopt/group ops are JIT-lowered (VM≡JIT parity), so the old jit=Off forcing \
         is gone; got jit={:?}",
        cfg.jit,
    );
    assert!(
        !cfg.checked_intrinsics,
        "an explicit --jit enable turns checked intrinsics off (the ordinary \
         jit-vs-checked resolution — the ownership flag no longer forces it)",
    );
    assert!(
        !cfg.mlir.enabled() && matches!(cfg.wasm, WasmPolicy::Off),
        "MLIR/WASM still trail the region-ownership lowering and stay forced off \
         (mlir={:?}, wasm={:?})",
        cfg.mlir,
        cfg.wasm,
    );
}

/// `--region-ownership` runs on the checked-on (native-Call) production path: the
/// funnel face keys the store adopt at the funnel call site (region-model.md
/// § "The funnel adopt — the checked-on store face"), so the old
/// checked-intrinsics=off forcing is gone and the CLI default (checked on)
/// stands. Counterfactual: before the funnel face, the ownership branch clobbered
/// `checked_intrinsics` to false and this assertion was RED.
#[test]
fn region_ownership_keeps_checked_intrinsics_on() {
    let (cfg, _rest) = Config::parse(&["--region-ownership".to_string()]).expect("parses");
    assert!(cfg.region_ownership, "the forest flag is set");
    assert!(
        cfg.checked_intrinsics,
        "--region-ownership alone must leave checked intrinsics at the CLI default \
         (on) — the funnel-adopt serves the production store path",
    );
    // Checked-on still forces the optimizing tiers off (the ordinary
    // checked normalization), and MLIR/WASM stay off regardless.
    assert!(
        !cfg.jit.enabled() && !cfg.mlir.enabled() && matches!(cfg.wasm, WasmPolicy::Off),
        "checked-on forces jit/mlir off; wasm trails the ownership lowering \
         (jit={:?}, mlir={:?}, wasm={:?})",
        cfg.jit,
        cfg.mlir,
        cfg.wasm,
    );
}
