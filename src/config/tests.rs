// audited: 2026-09-29
//! Pins the configuration: per-instance trace cells, the tier a build starts, the
//! refused flags and the page size.
//!
//! docs/config.md

use super::*;
use crate::value::fiberheap::pagepool::base_page;

/// Trace state is per-instance: a `RuntimeConfig` reads and writes its own
/// [`TraceCell`], so a diagnostic toggle on one instance never reaches another.
/// The test runner relies on this to keep a `--trace=`-heavy file from bleeding
/// into a parallel file's run. The counter-factual: one process-global atomic,
/// where any instance's `set_trace` flips the bit every off-VM reader sees.
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
/// works when named explicitly. The counter-factual: a keyword added with a
/// bit, a `from_name` entry and a `--help` line, and left out of the array.
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
    // Region/free diagnostics, the teardown residue dump, boot-phase timing,
    // the post-boot census, the park trace, and the syncjit compile mode:
    // functional today but checked via the string `has_trace` (cold free
    // paths in fiberheap/freelog.rs; the teardown in runtime.rs; the phase
    // marks and census in trace.rs; the park/resume seam; the submit path in
    // vm/jit_entry.rs), so they deliberately carry no `trace_bits` entry.
    const STRING_TRACED: &[&str] = &[
        "free",
        "guardfree",
        "freebt",
        "scrub",
        "residue",
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

fn parse_args(args: &[&str]) -> Result<Config, String> {
    let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    Config::parse(&owned).map(|(c, _)| c)
}

// ── The tier a build starts from (docs/config.md) ──

/// The binary and the embedding library start from one JIT policy.
///
/// The counter-factual: `Config::parse` overrides the struct `Default` with
/// `Off`, so `elle script.lisp` runs interpreted forever while a host calling
/// `Config::default()` compiles. Two answers to one question, and whichever
/// document a reader found was wrong half the time.
#[test]
fn the_cli_starts_the_jit_where_the_library_does() {
    assert_eq!(
        parse_args(&[]).unwrap().jit,
        Config::default().jit,
        "no flag must mean what the struct Default means"
    );
    assert_eq!(parse_args(&[]).unwrap().mlir, Config::default().mlir);
}

/// The JIT is the tier of a build that carries no other: it compiles a
/// function once the function has been called ten times.
#[cfg(all(feature = "jit", not(feature = "mlir"), not(feature = "wasm")))]
#[test]
fn a_jit_build_starts_the_jit_adaptive() {
    assert_eq!(Config::default().jit, JitPolicy::Adaptive { threshold: 10 });
}

/// A build carries one optimizing tier, and MLIR and WebAssembly each replace
/// the JIT. The counter-factual: the JIT default was written unconditionally,
/// so an `mlir` build ran both tiers and no build was one implementation.
#[cfg(any(feature = "mlir", feature = "wasm", not(feature = "jit")))]
#[test]
fn a_build_without_the_jit_tier_starts_it_off() {
    assert_eq!(Config::default().jit, JitPolicy::Off);
}

/// MLIR is the tier of an `mlir` build, unless WebAssembly replaces it.
#[cfg(all(feature = "mlir", not(feature = "wasm")))]
#[test]
fn an_mlir_build_starts_mlir_adaptive() {
    assert_eq!(
        Config::default().mlir,
        MlirPolicy::Adaptive { threshold: 10 }
    );
}

/// A build without the MLIR tier has nothing to start. The counter-factual:
/// the struct `Default` answered `Adaptive` in every build, and only the CLI
/// turned it off, so an embedding host asked a tier the build did not carry.
#[cfg(not(all(feature = "mlir", not(feature = "wasm"))))]
#[test]
fn a_build_without_the_mlir_tier_starts_it_off() {
    assert_eq!(Config::default().mlir, MlirPolicy::Off);
}

// ── The flags a user build no longer has (docs/config.md) ──

/// A flag that chose a tier, a backend or a pass is gone: the build chooses
/// them, and no user runs a matrix of runtimes. Before the program such a flag
/// is an unknown option, refused by name. The counter-factual is worse than an
/// error: a flag that no longer parses would otherwise become the program's
/// name, and `elle` would report a missing file called `--jit=off`.
#[test]
fn a_removed_flag_is_an_unknown_option() {
    for flag in [
        "--jit=off",
        "--jit=eager",
        "--jit=0",
        "--mlir=eager",
        "--anf=off",
        "--no-uring",
        "--stats",
        "--flip=on",
    ] {
        let err = parse_args(&[flag, "prog.lisp"]).unwrap_err();
        assert!(
            err.contains("unknown option") && err.contains(flag),
            "{flag} must be refused as an unknown option, got {err:?}"
        );
    }
}

/// After the program, an argument belongs to the program, whatever it looks
/// like (docs/config.md).
#[test]
fn an_unknown_flag_after_the_program_is_the_programs() {
    let owned: Vec<String> = ["prog.lisp", "--jit=off"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let (_, rest) = Config::parse(&owned).unwrap();
    assert_eq!(rest, owned, "the program keeps every argument after it");
}

/// `--wasm=` exists only where the backend does.
#[cfg(not(feature = "wasm"))]
#[test]
fn the_wasm_flag_is_unknown_without_the_wasm_feature() {
    let err = parse_args(&["--wasm=full", "prog.lisp"]).unwrap_err();
    assert!(err.contains("unknown option"), "got {err:?}");
}

#[cfg(feature = "wasm")]
#[test]
fn the_wasm_flag_sets_the_policy_in_a_wasm_build() {
    assert_eq!(parse_args(&["--wasm=full"]).unwrap().wasm, WasmPolicy::Full);
}

/// `--dump=stats` runs the program and prints statistics at its end; every
/// other dump keyword prints an artifact and runs nothing. So `stats` sets the
/// statistics switch and stays out of the dump set, whose non-emptiness is what
/// stops a run.
#[test]
fn dump_stats_runs_the_program() {
    let alone = parse_args(&["--dump=stats"]).unwrap();
    assert!(alone.stats, "--dump=stats turns statistics on");
    assert!(
        alone.dump.is_empty(),
        "--dump=stats alone must still run the program, got {:?}",
        alone.dump
    );
    let both = parse_args(&["--dump=lir,stats"]).unwrap();
    assert!(both.stats);
    assert_eq!(
        both.dump,
        std::collections::HashSet::from(["lir".to_string()]),
        "a stage beside stats still dumps and exits"
    );
}

// ── `--region-page-size` (docs/impl/region/model.md) ──

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
