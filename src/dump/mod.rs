//! In-process rendering of the compiler's `--dump` artifacts.
//!
//! `elle --dump=KIND` (see `main.rs::run_dump`) runs the compiler up to a stage
//! and prints the artifact to stdout, then exits. The agent-first test runner
//! (`src/test.lisp`, docs/test-runner.md) needs those same artifacts *in
//! process* — captured per form and written to the on-disk CAS — so the agent
//! can query the LIR of a failing form without re-running `--dump=lir`.
//!
//! This module is the single source of truth for the artifact *bodies* (the
//! text under each `;; ── kind ──` banner). `main.rs` prints the banner and then
//! the body produced here; the `compile/dumps` primitive
//! (`primitives::compile`, dispatched in `vm::signal`) returns the bodies as a
//! struct of `{kind => string}`. Keeping both paths on these functions means the
//! captured artifact is byte-identical to what `--dump` prints.
//!
//! Rendering mirrors `run_dump` exactly so the `tests/integration/dump_cli.rs`
//! markers (`block0:`, `←`, `→`, `capture_params_mask=`, `eligible=`, …) are
//! preserved.

mod escape;
pub use escape::escape_module;

use crate::lir::LirModule;
use crate::pipeline::CompileCtx;
use crate::symbol::SymbolTable;
use std::collections::BTreeMap;
use std::fmt::Write;

/// The dump kinds `render_all` attempts, in pipeline order. `git`/SPIR-V (mlir-
/// gated) and the runtime `stats` view are intentionally out of v1 (see
/// docs/test-runner.md § CAS asset capture).
pub const KINDS: &[&str] = &[
    "ast", "fhir", "defuse", "regions", "hir", "lir", "cfg", "dfa", "jit", "escape",
];

/// Compile `contents` once and render every available dump kind, returning the
/// non-empty artifact bodies keyed by kind. A stage that fails to compile or
/// produces nothing is omitted — the map only carries artifacts that exist, so
/// the caller can attach exactly those. The `fhir`/`defuse`/`regions` stages
/// share one front-end compile; `hir`/`lir`/`cfg`/`dfa`/`jit`/`escape` share one
/// pipeline compile (mirroring `run_dump`'s two compile passes).
pub fn render_all(
    contents: &str,
    source_name: &str,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();

    // Rendering is a pure diagnostic: it must not leave the process-global signal
    // registry mutated. A `(signal :kw)` declaration registers at compile time, so
    // without bracketing, the fhir front-end below would register the signal and
    // the lowered pipeline (a SECOND compile of the same source) would then collide
    // ("already registered") — silently dropping every lowered stage — and the
    // leftover registration would later collide with the runner's whole-module
    // compile of the same file. Restore the registry to this baseline before each
    // internal compile and on exit so render_all is registry-neutral.
    let registry_baseline = crate::signals::registry::snapshot_registry();

    if let Ok(s) = render_ast(contents, source_name) {
        if !s.is_empty() {
            out.insert("ast".to_string(), s);
        }
    }

    // fhir / defuse / regions share the functionalized-HIR front-end. Keep the
    // artifacts (held in `fhir`) — the `escape` dump below reuses this HIR/arena.
    crate::signals::registry::restore_registry(registry_baseline.clone());
    let fhir = crate::pipeline::compile_file_to_fhir(contents, symbols, cctx, source_name).ok();
    if let Some((hir, arena)) = &fhir {
        out.insert(
            "fhir".to_string(),
            crate::hir::display::display_hir(hir, arena, Some(symbols)),
        );
        let info = crate::hir::analyze_dataflow(hir);
        out.insert(
            "defuse".to_string(),
            crate::hir::format_dataflow(&info, arena, Some(symbols)),
        );
        let rinfo = crate::hir::analyze_regions(hir, arena);
        out.insert(
            "regions".to_string(),
            crate::hir::format_regions(&rinfo, arena, Some(symbols)),
        );
    }

    // hir / lir / cfg / dfa / jit / escape share the lowered-module compile.
    crate::signals::registry::restore_registry(registry_baseline.clone());
    let module = crate::pipeline::compile_file_to_lir(contents, symbols, cctx, source_name, 0).ok();
    if let Some(module) = &module {
        out.insert("hir".to_string(), hir_module(module));
        out.insert("lir".to_string(), lir_module(module));
        out.insert("cfg".to_string(), cfg_module(module));
        out.insert("dfa".to_string(), dfa_module(module));
        out.insert("jit".to_string(), jit_module(module));
    }

    // escape — the normalized escape snapshot (the golden's input; see
    // `escape.rs`). It needs the fhir HIR/arena AND the lowered module's region
    // instructions, plus region inference run with the real call classification:
    // the bare `analyze_regions` used for the `regions` dump above omits the
    // native effects the lowerer actually consumes, so escape facts (tail
    // regions, escape-return sites) would diverge from what was emitted.
    if let (Some((hir, arena)), Some(module)) = (&fhir, &module) {
        let pc = crate::lir::intrinsics::PrimitiveClassification::new(cctx.primitive_meta());
        let rinfo = crate::hir::analyze_regions_with(hir, arena, pc.call_classification.clone());
        // Escape is the authority the snapshot's return-frontier section reads; run
        // it with the same real classification the solver and lowerer consumed.
        let escape = crate::hir::analyze_escape(hir, arena, &pc.call_classification);
        out.insert(
            "escape".to_string(),
            escape::escape_module(hir, arena, &escape, &rinfo, module, Some(symbols)),
        );
    }

    // Net-neutral: undo any registrations the internal compiles left behind.
    crate::signals::registry::restore_registry(registry_baseline);

    out
}

/// AST — parsed syntax forms (one per line), the cheapest stage.
pub fn render_ast(contents: &str, source_name: &str) -> Result<String, String> {
    // A dump renders and discards: its own heap, freed when this returns.
    let mut home = crate::syntax::SyntaxHeap::new();
    let forms = crate::reader::read_syntax_all_for(home.arena(), contents, source_name)?;
    let mut s = String::new();
    for form in &forms {
        let _ = writeln!(s, "{}", form);
    }
    Ok(s)
}

/// HIR overview — per function: a header line plus its pre-expansion syntax.
/// Mirrors `main.rs::run_dump`'s `hir` branch (closure tag is `closure[i-1]`
/// across the `once(entry).chain(closures)` enumeration).
pub fn hir_module(module: &LirModule) -> String {
    let mut s = String::new();
    for (i, f) in std::iter::once(&module.entry)
        .chain(module.closures.iter())
        .enumerate()
    {
        let tag = if i == 0 {
            "entry".to_string()
        } else {
            format!("closure[{}]", i - 1)
        };
        let name = f.name.as_deref().unwrap_or("<anon>");
        let _ = writeln!(
            s,
            "; {} {} (arity={}, signal={:?})",
            tag, name, f.arity, f.signal
        );
        if let Some(origin) = &f.origin {
            let _ = writeln!(s, "; at {}", origin);
        }
    }
    s
}

/// LIR — blocks, instructions, and terminators per function. Mirrors
/// `main.rs::print_lir_function`.
pub fn lir_module(module: &LirModule) -> String {
    let mut s = String::new();
    lir_function(&mut s, "entry", &module.entry);
    for (i, f) in module.closures.iter().enumerate() {
        lir_function(&mut s, &format!("closure[{}]", i), f);
    }
    s
}

fn lir_function(s: &mut String, tag: &str, f: &crate::lir::LirFunction) {
    let name = f.name.as_deref().unwrap_or("<anon>");
    let _ = writeln!(
        s,
        "; {} {} (arity={}, signal={:?}, regs={}, locals={})",
        tag, name, f.arity, f.signal, f.num_regs, f.num_locals
    );
    for block in &f.blocks {
        let _ = writeln!(s, "  {}:", block.label);
        for si in &block.instructions {
            let _ = writeln!(s, "    {}", si.instr);
        }
        let _ = writeln!(s, "    -> {:?}", block.terminator.terminator);
    }
    let _ = writeln!(s);
}

/// CFG — block successor edges per function. Mirrors `main.rs::print_cfg_function`.
pub fn cfg_module(module: &LirModule) -> String {
    let mut s = String::new();
    cfg_function(&mut s, "entry", &module.entry);
    for (i, f) in module.closures.iter().enumerate() {
        cfg_function(&mut s, &format!("closure[{}]", i), f);
    }
    s
}

fn cfg_function(s: &mut String, tag: &str, f: &crate::lir::LirFunction) {
    use crate::lir::Terminator;
    let name = f.name.as_deref().unwrap_or("<anon>");
    let _ = writeln!(s, "; {} {}", tag, name);
    let _ = writeln!(s, "  entry: {}", f.entry);
    for block in &f.blocks {
        let succs: Vec<String> = match &block.terminator.terminator {
            Terminator::Jump(l) => vec![l.to_string()],
            Terminator::Branch {
                then_label,
                else_label,
                ..
            } => vec![then_label.to_string(), else_label.to_string()],
            Terminator::Emit { resume_label, .. } => vec![resume_label.to_string()],
            Terminator::Return(_) | Terminator::Unreachable => vec![],
        };
        let _ = writeln!(s, "  {} → [{}]", block.label, succs.join(", "));
    }
    let _ = writeln!(s);
}

/// DFA — per-function signal + capture-mask summary (`dfa_function` emits
/// `signal=` / `capture_params_mask=` / `capture_locals_mask=`).
pub fn dfa_module(module: &LirModule) -> String {
    let mut s = String::new();
    dfa_function(&mut s, "entry", &module.entry);
    for (i, f) in module.closures.iter().enumerate() {
        dfa_function(&mut s, &format!("closure[{}]", i), f);
    }
    s
}

fn dfa_function(s: &mut String, tag: &str, f: &crate::lir::LirFunction) {
    let name = f.name.as_deref().unwrap_or("<anon>");
    let _ = writeln!(
        s,
        "; {} {}: signal={:?} \
         capture_params_mask=0x{:x} capture_locals_mask=0x{:x}",
        tag, name, f.signal, f.capture_params_mask, f.capture_locals_mask,
    );
}

/// JIT — per-function eligibility (a polymorphic `propagates` mask is
/// ineligible). Mirrors `main.rs::print_jit_candidates`.
pub fn jit_module(module: &LirModule) -> String {
    let mut s = String::new();
    let mut report = |tag: &str, f: &crate::lir::LirFunction| {
        let eligible = f.signal.propagates == 0;
        let _ = writeln!(
            s,
            "; {} {}: signal={{bits={:?}, propagates=0b{:b}}} eligible={}",
            tag,
            f.name.as_deref().unwrap_or("<anon>"),
            f.signal.bits,
            f.signal.propagates,
            eligible,
        );
    };
    report("entry", &module.entry);
    for (i, f) in module.closures.iter().enumerate() {
        report(&format!("closure[{}]", i), f);
    }
    s
}

#[cfg(test)]
mod tests;
