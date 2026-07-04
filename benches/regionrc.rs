//! Compile-time RC-coalescing benchmark — the measured win of compile-time
//! region selection (docs/impl/region-rules.md § "Compile-time region selection
//! (coalescing)" / "Self-edge elimination").
//!
//! Reports, across real compilations, how many region-mints the lowerer resolved
//! to a static slot (the value→slot reduction of transform 1) versus left
//! value-resolved at the honest dynamic boundary, plus the merge-induced
//! self-edges transform 2 eliminated. Like `benches/memory.rs` this is a
//! *reporting* bench: it prints counts and asserts nothing — "the win is
//! measured, not asserted".
//!
//! Run with: cargo bench --bench regionrc
//!
//! The numbers come from the thread-local instrument in
//! `elle::lir::lower::rcstats`, which the lowerer bumps at each coalescing-
//! candidate site (the decision is not recoverable from the final LIR). Compiling
//! runs under the library-default config (`checked_intrinsics = false`), so
//! `%pair` survives as a `Pair` intrinsic and the builder-idiom merge — hence
//! transform 2 — can fire (CLI default `checked_intrinsics = true` dissolves
//! `%pair` to a native call and leaves the merge inert; see region-model.md
//! § Merging).

use std::panic::{catch_unwind, AssertUnwindSafe};

use elle::lir::lower::rcstats::{self, RcCoalesceStats};
use elle::pipeline::{compile, compile_file};
use elle::runtime::Runtime;

/// Print a stats block under a heading.
fn report(name: &str, s: &RcCoalesceStats) {
    let frac = s
        .slot_fraction()
        .map(|f| format!("{:.1}%", f * 100.0))
        .unwrap_or_else(|| "n/a".to_string());
    println!("  {name}");
    println!(
        "    transform 1 (value→slot): {} coalesced / {} candidate mints  ({frac} slot-resolved)",
        s.coalesced(),
        s.coalesced() + s.value_resolved(),
    );
    println!(
        "      return mints   : {} slot / {} value",
        s.return_mint_slot, s.return_mint_value
    );
    println!(
        "      reassign stores: {} slot / {} value",
        s.reassign_store_slot, s.reassign_store_value
    );
    println!(
        "      captured inits : {} slot / {} value",
        s.captured_init_slot, s.captured_init_value
    );
    println!(
        "    transform 2: {} merge-induced self-edges eliminated",
        s.self_edges_eliminated
    );
}

/// Measure the coalescing the stdlib load performs. Building a `Runtime` compiles
/// core.lisp, the prelude, and the stdlib — the largest real Elle compilation
/// unit, and a representative body of the builder/closure/export code coalescing
/// acts on.
fn measure_stdlib_load() -> RcCoalesceStats {
    rcstats::reset();
    let mut rt = Runtime::new();
    let stats = rcstats::take();
    rt.teardown();
    stats
}

/// Compile every `tests/elle/*.lisp` against one stdlib-loaded runtime,
/// accumulating the coalescing decisions. Compile-only (no execution), so corpus
/// `def`s do not persist into the shared context. Files that fail to compile
/// standalone (FFI/import/home-path dependencies) or panic the compiler are
/// skipped and counted. Returns `(stats, compiled, skipped)`.
fn measure_corpus() -> (RcCoalesceStats, usize, usize) {
    let mut rt = Runtime::new();
    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir("tests/elle")
        .expect("tests/elle corpus directory")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "lisp"))
        .collect();
    paths.sort();

    rcstats::reset();
    let (mut compiled, mut skipped) = (0usize, 0usize);
    for path in &paths {
        let name = path.to_string_lossy().to_string();
        let Ok(source) = std::fs::read_to_string(path) else {
            skipped += 1;
            continue;
        };
        // A corpus file may fail to compile standalone (an unresolved import) or,
        // worse, panic an analysis pass — neither should abort the sweep. The
        // panic hook is silenced for the duration so a skipped file leaves no
        // noise in the report.
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let (_, symbols, cctx) = rt.parts();
            compile_file(&source, symbols, cctx, &name)
        }));
        std::panic::set_hook(prev_hook);
        match outcome {
            Ok(Ok(_)) => compiled += 1,
            _ => skipped += 1,
        }
    }
    let stats = rcstats::take();
    rt.teardown();
    (stats, compiled, skipped)
}

/// A deterministic transform-2 witness: the discarded nested literal merges the
/// inner pair into the outer and drops the intra-region self-edge — independent
/// of whatever the corpus happens to contain, so the bench always exercises both
/// transforms. Compiled against a primitives-only runtime (no stdlib needed for
/// `%pair`/literals).
fn measure_builder_idiom() -> RcCoalesceStats {
    let mut rt = Runtime::without_stdlib();
    rcstats::reset();
    let (_, symbols, cctx) = rt.parts();
    let _ = compile(
        "(begin (%pair (%pair 1 2) 3) nil)",
        symbols,
        cctx,
        "<builder>",
    )
    .expect("builder idiom compiles");
    let stats = rcstats::take();
    rt.teardown();
    stats
}

fn main() {
    println!();
    println!("region RC coalescing — the measured win (verona/5a)");
    println!();

    let stdlib = measure_stdlib_load();
    report("stdlib load (core + prelude + stdlib)", &stdlib);
    println!();

    let (corpus, compiled, skipped) = measure_corpus();
    report(
        &format!("tests/elle corpus ({compiled} compiled, {skipped} skipped)"),
        &corpus,
    );
    println!();

    let builder = measure_builder_idiom();
    report("builder idiom (begin (%pair (%pair 1 2) 3) nil)", &builder);
    println!();
}
