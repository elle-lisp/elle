use elle::pipeline::{compile_file, CompileCtx};
use elle::repl::Repl;
use elle::runtime::Runtime;
use elle::{SymbolTable, VM};
use std::env;
use std::fs;
use std::io::{self, Read};

mod help;
use help::print_help;
mod errors;
use errors::{format_error_json, format_runtime_error, parse_compilation_error};

fn run_stdin(vm: &mut VM, symbols: &mut SymbolTable, cctx: &mut CompileCtx) -> Result<(), String> {
    let mut contents = String::new();
    io::stdin().read_to_string(&mut contents).map_err(|e| {
        let msg = format!("Failed to read stdin: {}", e);
        eprintln!("✗ {}", msg);
        msg
    })?;

    run_source(&contents, "<stdin>", vm, symbols, cctx)
}

fn run_file(
    filename: &str,
    vm: &mut VM,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
) -> Result<(), String> {
    let mut contents = fs::read_to_string(filename).map_err(|e| {
        let msg = format!("{}: {}", filename, e);
        eprintln!("✗ {}", msg);
        msg
    })?;

    // Strip shebang if present (e.g., #!/usr/bin/env elle)
    if contents.starts_with("#!") {
        contents = contents.lines().skip(1).collect::<Vec<_>>().join("\n");
    }

    run_source(&contents, filename, vm, symbols, cctx)
}

/// Implementation of `--dump=...`. Each requested stage prints a banner
/// followed by the artifact. Stages run in pipeline order (git, ast, hir,
/// lir, cfg, dfa, jit), so asking for multiple stages gives a coherent
/// top-to-bottom dump of the compiler.
fn run_dump(
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
        let (hir, arena, names) =
            elle::pipeline::compile_file_to_fhir(contents, symbols, cctx, source_name).map_err(
                |e| {
                    eprintln!("{}", e);
                    e
                },
            )?;
        println!("{}", elle::hir::display::display_hir(&hir, &arena, &names));
    }

    if cfg.dump.contains("defuse") {
        println!(";; ── defuse (HIR dataflow) ──────────────────────────────────");
        let (hir, arena, names) =
            elle::pipeline::compile_file_to_fhir(contents, symbols, cctx, source_name).map_err(
                |e| {
                    eprintln!("{}", e);
                    e
                },
            )?;
        let info = elle::hir::analyze_dataflow(&hir);
        print!("{}", elle::hir::format_dataflow(&info, &arena, &names));
    }

    if cfg.dump.contains("regions") {
        println!(";; ── regions (Tofte-Talpin region inference) ─────────────────");
        let (hir, arena, names) =
            elle::pipeline::compile_file_to_fhir(contents, symbols, cctx, source_name).map_err(
                |e| {
                    eprintln!("{}", e);
                    e
                },
            )?;
        let info = elle::hir::analyze_regions(&hir, &arena);
        print!("{}", elle::hir::format_regions(&info, &arena, &names));
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
        let (hir, arena, names) =
            elle::pipeline::compile_file_to_fhir(contents, symbols, cctx, source_name).map_err(
                |e| {
                    eprintln!("{}", e);
                    e
                },
            )?;
        let pc =
            elle::lir::intrinsics::PrimitiveClassification::new(symbols, cctx.primitive_meta());
        let rinfo = elle::hir::analyze_regions_with(&hir, &arena, pc.call_classification.clone());
        let escape = elle::hir::analyze_escape(&hir, &arena, &pc.call_classification);
        print!(
            "{}",
            elle::dump::escape_module(&hir, &arena, &names, &escape, &rinfo, &module)
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
                .chunks_exact(4)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
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

// The `lir`/`cfg`/`dfa`/`jit` artifact bodies are rendered by `elle::dump`
// (the single source of truth shared with the in-process `compile/dumps`
// primitive); `run_dump` prints each banner and then that body.

/// Run Elle source code from a string.
/// Only prints non-nil results.
/// The agent-first test runner, embedded at build time. See docs/test-runner.md.
const TEST_RUNNER_SRC: &str = include_str!("test.lisp");

/// `elle test ...` — set up a full VM and run the embedded runner with the
/// post-`test` arguments exposed to it as the program argv (via `(sys/argv)`).
/// The runner calls `(os/exit ...)` itself with the gate code; the Ok/Err
/// mapping here is the fallback if it returns without exiting.
fn run_test_subcommand(sub_args: Vec<String>) -> i32 {
    // Split off the global config flags (`--trace=...`, `--stats`) so the
    // embedded runner's VM (and the off-VM free-log / page-claim histogram)
    // honour them; the rest become the runner's argv. The runner itself does not
    // interpret these, so without this they would be slurped as corpus file
    // paths. (Runner-owned `--summary`/`--query`/… stay in `sub_args`.)
    let (config_flags, sub_args): (Vec<String>, Vec<String>) = sub_args
        .into_iter()
        .partition(|a| a.starts_with("--trace=") || a == "--stats");
    let (config, _rest) = elle::config::Config::parse(&config_flags).unwrap_or_else(|e| {
        eprintln!("elle test: {}", e);
        std::process::exit(1);
    });
    elle::config::init(config);
    elle::io::init_process_signals();

    // One runtime, one teardown — the same lifecycle every entry path uses.
    let mut rt = Runtime::new();
    rt.vm().source_arg = "<test>".to_string();
    rt.vm().user_args = sub_args;

    let code = {
        let (vm, symbols, cctx) = rt.parts();
        match run_source(TEST_RUNNER_SRC, "src/test.lisp", vm, symbols, cctx) {
            Ok(_) => 0,
            Err(_) => 1,
        }
    };
    // The runner usually calls `(os/exit …)` itself (skipping Drop); on a
    // graceful return `rt`'s Drop runs the principled teardown sweep.
    code
}

fn run_source(
    contents: &str,
    source_name: &str,
    vm: &mut VM,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
) -> Result<(), String> {
    // --dump=...: run the compiler up to each requested stage, print the
    // artifact, and exit without executing.
    if !elle::config::get().dump.is_empty() {
        return run_dump(contents, source_name, symbols, cctx);
    }

    // WASM backend: compile and run through Wasmtime instead of bytecode VM
    #[cfg(feature = "wasm")]
    if elle::config::get().wasm_full() {
        let eval_fn = if elle::config::get().no_stdlib {
            elle::wasm::eval_wasm
        } else {
            elle::wasm::eval_wasm_with_stdlib
        };
        return match eval_fn(contents, source_name) {
            Ok(_) => Ok(()),
            Err(e) => {
                eprintln!("{}", e);
                Err(e)
            }
        };
    }

    // Compile file as a single letrec
    let result = match compile_file(contents, symbols, cctx, source_name) {
        Ok(r) => r,
        Err(e) => {
            let lerr = parse_compilation_error(&e);
            if elle::config::get().json {
                eprintln!("{}", format_error_json(&lerr));
            } else {
                eprintln!("{}", lerr.format_with_source());
            }
            return Err(e);
        }
    };

    // Debug: print bytecode if --debug is set
    if elle::config::get().has_trace("bytecode") {
        eprintln!(
            "{}",
            elle::compiler::format_bytecode_with_protos(&result.bytecode)
        );
    }

    match vm.execute_scheduled(&result.bytecode, symbols, cctx) {
        Ok(_) => {
            // Script mode is silent except for explicit output (display, etc.)
            Ok(())
        }
        Err(e) => {
            // A loud (gate! …) whose condition is unmet propagates an uncaught
            // :gated signal. That is an intentional SKIP, not a failure — report
            // the reason and exit 0, so gate! is a universal skip mechanism (the
            // same intent the test runner records as status=skip). Any other
            // uncaught error still fails.
            if let Some(reason) = vm.take_gated_exit_reason() {
                eprintln!("SKIP (gated): {}", reason);
                return Ok(());
            }
            eprintln!("{}", format_runtime_error(&e, symbols));
            Err("Errors encountered during execution".to_string())
        }
    }
}

fn run_repl(vm: &mut VM, symbols: &mut SymbolTable, cctx: &mut CompileCtx) -> bool {
    match Repl::new() {
        Ok(mut repl) => repl.run(vm, symbols, cctx),
        Err(e) => {
            eprintln!("✗ Failed to initialize readline: {}", e);
            Repl::run_fallback(vm, symbols, cctx)
        }
    }
}

#[cfg(feature = "jit")]
fn print_jit_stats(vm: &mut VM) {
    // Drain pending background compilations so stats are complete.
    vm.drain_jit_pending();
    let compiled = vm.jit_cache.len();
    let rejected = vm.jit_rejections.len();

    eprintln!("JIT stats:");
    eprintln!("  compiled: {}", compiled);
    eprintln!("  rejected: {}", rejected);

    if rejected > 0 {
        // Sort by call count ascending
        let mut entries: Vec<_> = vm.jit_rejections.iter().collect();
        entries.sort_by_key(|(ptr, _)| vm.closure_call_counts.get(ptr).copied().unwrap_or(0));

        for (ptr, info) in &entries {
            let name = info.name.as_deref().unwrap_or("<anon>");
            let calls = vm.closure_call_counts.get(ptr).copied().unwrap_or(0);
            eprintln!("    {:<24} {}  [called {}x]", name, info.reason, calls);
        }
    }
}

fn main() {
    // dlopen'd C++ plugins (e.g. oxigraph) allocate from glibc's static TLS
    // block at load time. glibc 2.39+ grows that reservation on demand, so no
    // up-front reservation is needed here. If plugin loading ever fails with
    // "cannot allocate memory in static TLS block", set
    // GLIBC_TUNABLES=glibc.rtld.optional_static_tls=65536 before launching elle.

    let args: Vec<String> = env::args().collect();

    // Subcommand dispatch — no VM setup needed for these
    match args.get(1).map(|s| s.as_str()) {
        Some("fmt") => {
            let sub_args: Vec<String> = args[2..].to_vec();
            let exit_code = elle::formatter::run::run(&sub_args);
            std::process::exit(exit_code);
        }
        Some("lint") => {
            let sub_args: Vec<String> = args[2..].to_vec();
            let exit_code = elle::lint::run::run(&sub_args);
            std::process::exit(exit_code);
        }
        Some("lsp") => {
            let exit_code = elle::lsp::run::run();
            std::process::exit(exit_code);
        }
        Some("rewrite") => {
            let sub_args: Vec<String> = args[2..].to_vec();
            let exit_code = elle::rewrite::run::run(&sub_args);
            std::process::exit(exit_code);
        }
        Some("test") => {
            // The runner is an Elle program (src/test.lisp). Unlike fmt/lint it
            // needs a full VM (sqlite FFI, stdlib, threads), so run the embedded
            // source with the post-`test` args handed to it as the program argv.
            let sub_args: Vec<String> = args[2..].to_vec();
            let exit_code = run_test_subcommand(sub_args);
            std::process::exit(exit_code);
        }
        _ => {}
    }

    // Interpreter mode — needs VM setup

    // Check for --help/-h first (before VM init)
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return;
    }

    let (config, remaining_args) = elle::config::Config::parse(&args[1..]).unwrap_or_else(|e| {
        eprintln!("elle: {}", e);
        std::process::exit(1);
    });
    elle::config::init(config);

    // Trap POSIX signals at startup, before any thread spawn. This
    // installs sigaction handlers for TERM/INT/QUIT/HUP (clean exit),
    // TSTP/TTIN/TTOU (raise SIGSTOP), CONT (consume), SIGPIPE (ignore),
    // and pthread_sigmask-blocks the absorb-set (USR1/USR2/CHLD/URG/
    // WINCH/ALRM) on the main thread. See `init_process_signals` in
    // src/io/sigfd.rs and docs/posix-signals.md for the full table.
    //
    // Must run before VM::new() because VM construction may spawn
    // worker threads (currently it doesn't, but the JIT worker spawned
    // later inherits whatever mask the main thread holds at its spawn
    // time). Workers' own `mask_all_signals_on_this_thread()` calls
    // are belt-and-suspenders but are not the primary defence.
    elle::io::init_process_signals();

    // One runtime drives every entry path (file / eval / stdin / REPL); its
    // Drop (or the explicit `teardown` below) runs the principled, RC-driven
    // teardown sweep (docs/impl/region/rules.md § "Teardown — every region frees").
    let mut rt = if elle::config::get().no_stdlib {
        Runtime::without_stdlib()
    } else {
        Runtime::new()
    };

    let mut had_errors = false;
    let mut files: Vec<String> = Vec::new();
    let mut eval_exprs: Vec<String> = Vec::new();
    let mut read_stdin = false;

    // remaining_args from Config::parse: file args, eval expressions (--eval:...), and user args after --.
    // Separate eval expressions from file args.
    for (i, arg) in remaining_args.iter().enumerate() {
        if let Some(expr) = arg.strip_prefix("--eval:") {
            eval_exprs.push(expr.to_string());
        } else if arg == "-" && files.is_empty() && eval_exprs.is_empty() {
            read_stdin = true;
            rt.vm().source_arg = "-".to_string();
            rt.vm().user_args = remaining_args[i + 1..].to_vec();
            break;
        } else if arg == "--" {
            rt.vm().user_args = remaining_args[i + 1..].to_vec();
            break;
        } else if files.is_empty() && eval_exprs.is_empty() {
            rt.vm().source_arg = arg.clone();
            files.push(arg.clone());
            // Everything after the first file arg goes to user_args
            rt.vm().user_args = remaining_args[i + 1..].to_vec();
            break;
        }
    }
    if eval_exprs.is_empty() && files.is_empty() && !read_stdin {
        // REPL mode: vm.source_arg stays "" and vm.user_args stays empty.
    } else if !eval_exprs.is_empty() && files.is_empty() && !read_stdin {
        rt.vm().source_arg = "<eval>".to_string();
    }

    if read_stdin {
        let (vm, symbols, cctx) = rt.parts();
        if run_stdin(vm, symbols, cctx).is_err() {
            had_errors = true;
        }
    } else if !eval_exprs.is_empty() {
        for expr in &eval_exprs {
            let (vm, symbols, cctx) = rt.parts();
            if run_source(expr, "<eval>", vm, symbols, cctx).is_err() {
                had_errors = true;
            }
        }
    } else if !files.is_empty() {
        for filename in &files {
            let (vm, symbols, cctx) = rt.parts();
            if run_file(filename, vm, symbols, cctx).is_err() {
                had_errors = true;
            }
        }
    } else {
        let (vm, symbols, cctx) = rt.parts();
        if run_repl(vm, symbols, cctx) {
            had_errors = true;
        }
    }

    let stats = elle::config::get().stats;
    if stats {
        #[cfg(feature = "jit")]
        print_jit_stats(rt.vm());
        let cvc = elle::lir::closure_value_const_count();
        if cvc > 0 {
            eprintln!("[stats] closure-valued ValueConsts serialized: {}", cvc);
        }
    }

    // Graceful exit on every path: run the principled teardown sweep explicitly
    // (so it happens before any `process::exit`, which would skip `rt`'s Drop)
    // and surface its observable result under `--stats`.
    let report = rt.teardown();
    if stats {
        eprintln!(
            "[stats] live regions after teardown: {} \
             (0 = clean; residue names open leaks)",
            report.live_regions
        );
    }

    if !read_stdin && files.is_empty() && eval_exprs.is_empty() {
        println!();
    }

    if had_errors {
        std::process::exit(1);
    }
}
