// audited: 2026-09-14
//! `--dump=STAGE[,STAGE,...]`: run the compiler up to each requested stage,
//! print the artifact, and exit without executing.
//!
//! The artifact bodies come from `elle::dump`, the source `compile/dumps`
//! shares, so the CLI and the in-process primitive render one thing. What
//! lives here is the stage selection, the banners, and the per-kind
//! re-derivation of the front-end inputs each stage needs.
//!
//! docs/config.md

use elle::pipeline::CompileCtx;
use elle::SymbolTable;

/// Implementation of `--dump=...`. Each requested stage prints a banner
/// followed by the artifact. Stages run in pipeline order (git, ast, hir,
/// lir, cfg, dfa, jit), so asking for multiple stages gives a coherent
/// top-to-bottom dump of the compiler.
pub(super) fn run_dump(
    contents: &str,
    source_name: &str,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
) -> Result<(), String> {
    use elle::config::dump_bits;
    let cfg = elle::config::get();

    // AST — parsed syntax forms (cheapest stage; no analyzer needed).
    let needs_ast = cfg.dump.contains("ast");
    if needs_ast {
        println!(";; ── ast ────────────────────────────────────────────────────");
        let ast = elle::dump::render_ast(contents, source_name).map_err(|e| {
            eprintln!("{}", e);
            e
        })?;
        print!("{}", ast);
    }

    // HIR / LIR / CFG / DFA / JIT / git (SPIR-V) all flow off
    // compile_file_to_lir. Only run the pipeline once if any of them are
    // requested.
    // FHIR — functionalized HIR (s-expression dump before lowering)
    if cfg.dump.contains("fhir") {
        println!(";; ── fhir (functionalized HIR) ──────────────────────────────");
        let (hir, arena) =
            elle::pipeline::compile_file_to_fhir(contents, symbols, cctx, source_name).map_err(
                |e| {
                    eprintln!("{}", e);
                    e
                },
            )?;
        println!(
            "{}",
            elle::hir::display::display_hir(&hir, &arena, Some(symbols))
        );
    }

    if cfg.dump.contains("defuse") {
        println!(";; ── defuse (HIR dataflow) ──────────────────────────────────");
        let (hir, arena) =
            elle::pipeline::compile_file_to_fhir(contents, symbols, cctx, source_name).map_err(
                |e| {
                    eprintln!("{}", e);
                    e
                },
            )?;
        let info = elle::hir::analyze_dataflow(&hir);
        print!(
            "{}",
            elle::hir::format_dataflow(&info, &arena, Some(symbols))
        );
    }

    if cfg.dump.contains("regions") {
        println!(";; ── regions (Tofte-Talpin region inference) ─────────────────");
        let (hir, arena) =
            elle::pipeline::compile_file_to_fhir(contents, symbols, cctx, source_name).map_err(
                |e| {
                    eprintln!("{}", e);
                    e
                },
            )?;
        let info = elle::hir::analyze_regions(&hir, &arena);
        print!(
            "{}",
            elle::hir::format_regions(&info, &arena, Some(symbols))
        );
    }

    let needs_pipeline = cfg.dump.iter().any(|k| {
        matches!(
            k.as_str(),
            "hir" | "lir" | "cfg" | "dfa" | "jit" | "git" | "escape"
        )
    });
    if !needs_pipeline {
        return Ok(());
    }

    let module = elle::pipeline::compile_file_to_lir(contents, symbols, cctx, source_name, 0)
        .map_err(|e| {
            eprintln!("{}", e);
            e
        })?;

    if cfg.dump.contains("hir") {
        println!(";; ── hir ────────────────────────────────────────────────────");
        print!("{}", elle::dump::hir_module(&module));
    }

    if cfg.dump.contains("lir") {
        println!(";; ── lir ────────────────────────────────────────────────────");
        print!("{}", elle::dump::lir_module(&module));
    }

    if cfg.dump.contains("cfg") {
        println!(";; ── cfg ────────────────────────────────────────────────────");
        print!("{}", elle::dump::cfg_module(&module));
    }

    if cfg.dump.contains("dfa") {
        println!(";; ── dfa ────────────────────────────────────────────────────");
        print!("{}", elle::dump::dfa_module(&module));
    }

    if cfg.dump.contains("jit") {
        println!(";; ── jit ────────────────────────────────────────────────────");
        print!("{}", elle::dump::jit_module(&module));
    }

    if cfg.dump.contains("git") {
        println!(";; ── git ────────────────────────────────────────────────────");
        print_spirv_module(&module);
    }

    if cfg.dump.contains("escape") {
        println!(";; ── escape (normalized escape snapshot) ─────────────────────");
        // Re-derive the front-end artifacts (run_dump compiles per kind) plus the
        // classification-aware region info — same inputs `render_all` feeds
        // `escape_module`, so the CLI and `compile/dumps :escape` agree.
        let (hir, arena) =
            elle::pipeline::compile_file_to_fhir(contents, symbols, cctx, source_name).map_err(
                |e| {
                    eprintln!("{}", e);
                    e
                },
            )?;
        let pc = elle::lir::intrinsics::PrimitiveClassification::new(cctx.primitive_meta());
        let rinfo = elle::hir::analyze_regions_with(&hir, &arena, pc.call_classification.clone());
        let escape = elle::hir::analyze_escape(&hir, &arena, &pc.call_classification);
        print!(
            "{}",
            elle::dump::escape_module(&hir, &arena, &escape, &rinfo, &module, Some(symbols))
        );
    }

    let _ = dump_bits::ALL; // keep import used even if a stage is added lazily
    Ok(())
}

/// Dump SPIR-V disassembly for each GPU-eligible closure. The "git" keyword
/// names this stage (a shorthand; it's the GPU codegen output).
fn print_spirv_module(module: &elle::lir::LirModule) {
    print_spirv_function("entry", &module.entry);
    for (i, f) in module.closures.iter().enumerate() {
        print_spirv_function(&format!("closure[{}]", i), f);
    }
}

#[cfg(feature = "mlir")]
fn print_spirv_function(tag: &str, f: &elle::lir::LirFunction) {
    let name = f.name.as_deref().unwrap_or("<anon>");
    println!("; {} {}", tag, name);
    if !f.is_gpu_eligible() {
        println!(";   (not GPU-eligible; skipped)");
        println!();
        return;
    }
    // Workgroup size of 1 is a safe default for dump purposes — users
    // selecting a workgroup size do so via vm/config at runtime.
    match elle::mlir::lower_to_spirv(f, 1) {
        Ok(bytes) => {
            println!(";   SPIR-V ({} bytes):", bytes.len());
            // Words are 32-bit in SPIR-V. Print as hex, 8 words per line.
            let words: Vec<u32> = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| u32::from_le_bytes(*c))
                .collect();
            for (i, chunk) in words.chunks(8).enumerate() {
                print!("  {:04x}:", i * 8);
                for w in chunk {
                    print!(" {:08x}", w);
                }
                println!();
            }
            println!();
        }
        Err(e) => {
            println!(";   SPIR-V lowering failed: {}", e);
            println!();
        }
    }
}

#[cfg(not(feature = "mlir"))]
fn print_spirv_function(tag: &str, f: &elle::lir::LirFunction) {
    let name = f.name.as_deref().unwrap_or("<anon>");
    println!("; {} {}", tag, name);
    println!(";   (SPIR-V dump requires the `mlir` feature)");
    println!();
    let _ = f;
}
