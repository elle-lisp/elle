use elle::context::{clear_vm_context, set_symbol_table, set_vm_context};
use elle::pipeline::compile_file;
use elle::repl::Repl;
use elle::{init_stdlib, register_primitives, SymbolTable, VM};
use std::env;
use std::fs;
use std::io::{self, Read};

fn print_help() {
    println!("Elle v1.0.0\n");
    println!("Usage: elle [file...] [-- args...]       Run files or start REPL");
    println!("       elle fmt [options] <file...>       Format source files");
    println!("       elle lint [options] <file|dir>... Static analysis");
    println!("       elle lsp                          Start language server");
    println!("       elle rewrite [options] <file...>  Source-to-source rewriting\n");
    println!("Options:");
    println!("  -h, --help            Show this help");
    println!("  -e, --eval EXPR       Evaluate expression");
    println!("  -                     Read from stdin");
    println!("  --dump=KW[,KW,...]    Dump compiler artifacts and exit. Keywords:");
    println!("                          ast  — parsed syntax forms");
    println!("                          hir  — resolved HIR");
    println!("                          fhir — functionalized HIR (s-expression)");
    println!("                          lir  — lowered LIR (SSA)");
    println!("                          jit  — JIT eligibility per function");
    println!("                          cfg  — per-function control-flow graph");
    println!("                          dfa  — dataflow / signal inference results");
    println!("                          git  — (reserved for SPIR-V output)");
    println!("  --dump=all            Dump every stage");
    println!("  --jit=POLICY          JIT policy: off, eager, adaptive (default), or integer N");
    println!("  --mlir=POLICY         MLIR policy: off, eager, adaptive (default), or integer N");
    println!("  --wasm=POLICY         WASM policy: off (default), full, lazy, or integer N");
    println!("  --flip=on|off         Insert FlipEnter/FlipSwap/FlipExit instructions");
    println!("                          (escape-analysis-gated rotation; default on)");
    println!("  --trace=KW[,KW,...]   Trace subsystems. Keywords:");
    println!("                          call, signal, compile, fiber, hir, lir,");
    println!("                          emit, jit, io, gc, import, macro, wasm,");
    println!("                          capture, arena, escape, bytecode");
    println!("  --trace=all           Trace everything");
    println!("  --stats               Print compilation stats on exit");
    println!("  --home=DIR            Module resolution root (env: ELLE_HOME)");
    println!("  --path=DIRS           Colon-separated module search path (env: ELLE_PATH)");
    println!("  --cache=DIR           Disk cache directory (env: ELLE_CACHE)");
    println!("  --json                JSON output on stderr\n");
    println!("Syntax:");
    println!("  .lisp             S-expression syntax (default)");
    println!("  .py               Python syntax");
    println!("  .js               JavaScript syntax");
    println!("  .lua              Lua syntax");
    println!("  .md               Literate markdown (```lisp blocks)");
}

/// Format a runtime error with symbol resolution
fn format_runtime_error(error: &str, symbols: &SymbolTable) -> String {
    // Check for SymbolId pattern and resolve it
    if let Some(start) = error.find("SymbolId(") {
        if let Some(end) = error[start..].find(')') {
            let id_str = &error[start + 9..start + end];
            if let Ok(id) = id_str.parse::<u32>() {
                let name = symbols
                    .name(elle::value::SymbolId(id))
                    .unwrap_or("<unknown>");
                let before = &error[..start];
                let after = &error[start + end + 1..];
                return format!("{}'{}'{}", before, name, after);
            }
        }
    }
    error.to_string()
}

/// Parse a compilation error string into an LError for structured display.
/// When the error has "file:line:col: message" format, extracts location.
/// Uses Generic kind so `description()` returns just the message without
/// an extra "Compile error:" prefix (the caller provides context).
fn parse_compilation_error(error: &str) -> elle::error::LError {
    // Try to extract location from "file:line:col: message" pattern
    if let Some(colon_idx) = error.find(": ") {
        let loc_part = &error[..colon_idx];
        let parts: Vec<&str> = loc_part.rsplitn(3, ':').collect();
        if parts.len() >= 2 {
            if let (Ok(col), Ok(line)) = (parts[0].parse::<usize>(), parts[1].parse::<usize>()) {
                let file = if parts.len() == 3 {
                    parts[2]
                } else {
                    "<unknown>"
                };
                let message = &error[colon_idx + 2..];
                return elle::error::LError::new(elle::error::ErrorKind::CompileError {
                    message: message.to_string(),
                })
                .with_location(elle::error::SourceLoc::new(file, line, col));
            }
        }
    }
    elle::error::LError::compile_error(error)
}

/// Format a compilation error as JSON for --json mode
fn format_error_json(error: &elle::error::LError) -> String {
    let (file, line, col) = match &error.location {
        Some(loc) => (loc.file.as_str(), loc.line, loc.col),
        None => ("<unknown>", 0, 0),
    };
    let (kind, message) = match &error.kind {
        elle::error::ErrorKind::UndefinedVariable {
            name, suggestions, ..
        } => {
            let msg = if suggestions.is_empty() {
                format!("undefined variable: {}", name)
            } else {
                format!(
                    "undefined variable: {} (did you mean: {}?)",
                    name,
                    suggestions.join(", ")
                )
            };
            ("undefined-variable", msg)
        }
        elle::error::ErrorKind::SignalMismatch {
            function,
            required_mask,
            actual_mask,
        } => (
            "signal-mismatch",
            format!(
                "function {} restricted to {} but body may emit {}",
                function, required_mask, actual_mask
            ),
        ),
        elle::error::ErrorKind::CompileError { message } => ("compile-error", message.clone()),
        elle::error::ErrorKind::SyntaxError { message, .. } => ("syntax-error", message.clone()),
        _ => ("error", error.description()),
    };
    format!(
        r#"{{"error":"compile-error","kind":"{}","file":"{}","line":{},"col":{},"message":"{}"}}"#,
        kind,
        file.replace('\\', "\\\\").replace('"', "\\\""),
        line,
        col,
        message.replace('\\', "\\\\").replace('"', "\\\""),
    )
}

fn run_stdin(vm: &mut VM, symbols: &mut SymbolTable) -> Result<(), String> {
    let mut contents = String::new();
    io::stdin().read_to_string(&mut contents).map_err(|e| {
        let msg = format!("Failed to read stdin: {}", e);
        eprintln!("✗ {}", msg);
        msg
    })?;

    run_source(&contents, "<stdin>", vm, symbols)
}

fn run_file(filename: &str, vm: &mut VM, symbols: &mut SymbolTable) -> Result<(), String> {
    let mut contents = fs::read_to_string(filename).map_err(|e| {
        let msg = format!("{}: {}", filename, e);
        eprintln!("✗ {}", msg);
        msg
    })?;

    // Strip shebang if present (e.g., #!/usr/bin/env elle)
    if contents.starts_with("#!") {
        contents = contents.lines().skip(1).collect::<Vec<_>>().join("\n");
    }

    run_source(&contents, filename, vm, symbols)
}

/// Implementation of `--dump=...`. Each requested stage prints a banner
/// followed by the artifact. Stages run in pipeline order (git, ast, hir,
/// lir, cfg, dfa, jit), so asking for multiple stages gives a coherent
/// top-to-bottom dump of the compiler.
fn run_dump(contents: &str, source_name: &str, symbols: &mut SymbolTable) -> Result<(), String> {
    use elle::config::dump_bits;
    let cfg = elle::config::get();

    // AST — parsed syntax forms (cheapest stage; no analyzer needed).
    let needs_ast = cfg.dump.contains("ast");
    if needs_ast {
        println!(";; ── ast ────────────────────────────────────────────────────");
        let forms = elle::reader::read_syntax_all_for(contents, source_name).map_err(|e| {
            eprintln!("{}", e);
            e
        })?;
        for form in &forms {
            println!("{}", form);
        }
    }

    // HIR / LIR / CFG / DFA / JIT / git (SPIR-V) all flow off
    // compile_file_to_lir. Only run the pipeline once if any of them are
    // requested.
    // FHIR — functionalized HIR (s-expression dump before lowering)
    if cfg.dump.contains("fhir") {
        println!(";; ── fhir (functionalized HIR) ──────────────────────────────");
        let (hir, arena, names) =
            elle::pipeline::compile_file_to_fhir(contents, symbols, source_name).map_err(|e| {
                eprintln!("{}", e);
                e
            })?;
        println!("{}", elle::hir::display::display_hir(&hir, &arena, &names));
    }

    let needs_pipeline = cfg
        .dump
        .iter()
        .any(|k| matches!(k.as_str(), "hir" | "lir" | "cfg" | "dfa" | "jit" | "git"));
    if !needs_pipeline {
        return Ok(());
    }

    let module =
        elle::pipeline::compile_file_to_lir(contents, symbols, source_name, 0).map_err(|e| {
            eprintln!("{}", e);
            e
        })?;

    if cfg.dump.contains("hir") {
        println!(";; ── hir ────────────────────────────────────────────────────");
        // No dedicated pretty-printer — use Debug. The per-function
        // `syntax` Rc on each LirFunction carries the pre-expansion form
        // for reference; we print that for a compact overview.
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
            println!(
                "; {} {} (arity={}, signal={:?})",
                tag, name, f.arity, f.signal
            );
            if let Some(syn) = &f.syntax {
                println!("{}", syn);
            }
        }
    }

    if cfg.dump.contains("lir") {
        println!(";; ── lir ────────────────────────────────────────────────────");
        print_lir_module(&module);
    }

    if cfg.dump.contains("cfg") {
        println!(";; ── cfg ────────────────────────────────────────────────────");
        print_cfg_module(&module);
    }

    if cfg.dump.contains("dfa") {
        println!(";; ── dfa ────────────────────────────────────────────────────");
        print_dfa_module(&module);
    }

    if cfg.dump.contains("jit") {
        println!(";; ── jit ────────────────────────────────────────────────────");
        print_jit_candidates(&module);
    }

    if cfg.dump.contains("git") {
        println!(";; ── git ────────────────────────────────────────────────────");
        print_spirv_module(&module);
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

fn print_lir_module(module: &elle::lir::LirModule) {
    print_lir_function("entry", &module.entry);
    for (i, f) in module.closures.iter().enumerate() {
        print_lir_function(&format!("closure[{}]", i), f);
    }
}

fn print_lir_function(tag: &str, f: &elle::lir::LirFunction) {
    let name = f.name.as_deref().unwrap_or("<anon>");
    println!(
        "; {} {} (arity={}, signal={:?}, regs={}, locals={})",
        tag, name, f.arity, f.signal, f.num_regs, f.num_locals
    );
    for block in &f.blocks {
        println!("  {}:", block.label);
        for si in &block.instructions {
            println!("    {}", si.instr);
        }
        println!("    -> {:?}", block.terminator.terminator);
    }
    println!();
}

fn print_cfg_module(module: &elle::lir::LirModule) {
    print_cfg_function("entry", &module.entry);
    for (i, f) in module.closures.iter().enumerate() {
        print_cfg_function(&format!("closure[{}]", i), f);
    }
}

fn print_cfg_function(tag: &str, f: &elle::lir::LirFunction) {
    use elle::lir::Terminator;
    let name = f.name.as_deref().unwrap_or("<anon>");
    println!("; {} {}", tag, name);
    println!("  entry: {}", f.entry);
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
        println!("  {} → [{}]", block.label, succs.join(", "));
    }
    println!();
}

fn print_dfa_module(module: &elle::lir::LirModule) {
    print_dfa_function("entry", &module.entry);
    for (i, f) in module.closures.iter().enumerate() {
        print_dfa_function(&format!("closure[{}]", i), f);
    }
}

fn print_dfa_function(tag: &str, f: &elle::lir::LirFunction) {
    let name = f.name.as_deref().unwrap_or("<anon>");
    println!(
        "; {} {}: signal={:?} rotation_safe={} result_immediate={} outward_heap_set={} \
         capture_params_mask=0x{:x} capture_locals_mask=0x{:x}",
        tag,
        name,
        f.signal,
        f.rotation_safe,
        f.result_is_immediate,
        f.has_outward_heap_set,
        f.capture_params_mask,
        f.capture_locals_mask,
    );
}

fn print_jit_candidates(module: &elle::lir::LirModule) {
    // JIT rejects polymorphic closures (those whose signal's `propagates`
    // bits are set — signal depends on a caller-supplied function). Silent
    // and statically-yielding functions are eligible. Callable mutability
    // (captures) is handled separately by the JIT.
    let report = |tag: &str, f: &elle::lir::LirFunction| {
        let eligible = f.signal.propagates == 0;
        println!(
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
}

/// Run Elle source code from a string.
/// Only prints non-nil results.
fn run_source(
    contents: &str,
    source_name: &str,
    vm: &mut VM,
    symbols: &mut SymbolTable,
) -> Result<(), String> {
    // --dump=...: run the compiler up to each requested stage, print the
    // artifact, and exit without executing.
    if !elle::config::get().dump.is_empty() {
        return run_dump(contents, source_name, symbols);
    }

    // WASM backend: compile and run through Wasmtime instead of bytecode VM
    #[cfg(feature = "wasm")]
    if elle::config::get().wasm_full {
        let no_stdlib = elle::config::get().wasm_no_stdlib;
        let eval_fn = if no_stdlib {
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
    let result = match compile_file(contents, symbols, source_name) {
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
    if elle::config::get().debug {
        eprintln!(
            "{}",
            elle::compiler::format_bytecode_with_constants(
                &result.bytecode.instructions,
                &result.bytecode.constants
            )
        );
    }

    match vm.execute_scheduled(&result.bytecode, symbols) {
        Ok(_) => {
            // Script mode is silent except for explicit output (display, etc.)
            Ok(())
        }
        Err(e) => {
            eprintln!("{}", format_runtime_error(&e, symbols));
            Err("Errors encountered during execution".to_string())
        }
    }
}

fn run_repl(vm: &mut VM, symbols: &mut SymbolTable) -> bool {
    match Repl::new() {
        Ok(mut repl) => repl.run(vm, symbols),
        Err(e) => {
            eprintln!("✗ Failed to initialize readline: {}", e);
            Repl::run_fallback(vm, symbols)
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
    // DISABLED (2026-04-18): static TLS re-exec hack for dlopen'd plugins.
    //
    // Was: set GLIBC_TUNABLES=glibc.rtld.optional_static_tls=65536 and
    // re-exec the process so the dynamic linker sees it before main().
    // This reserved 64KB of optional static TLS for C++ plugins loaded
    // via dlopen (e.g. oxigraph).
    //
    // Removed because:
    //   - Linux/glibc only; no-op on musl, Bionic (Android), macOS
    //   - The re-exec changes PID, breaking strace/gdb workflows
    //   - current_exe() can fail in chroots/containers
    //   - MCP server and all plugins load fine without it on glibc 2.39+
    //
    // If plugin loading fails with "cannot allocate memory in static TLS
    // block", restore the block from git (commit 9ed7b880) or set
    // GLIBC_TUNABLES manually before launching elle.

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

    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();

    let _signals = register_primitives(&mut vm, &mut symbols);

    set_symbol_table(&mut symbols as *mut SymbolTable);

    init_stdlib(&mut vm, &mut symbols);

    set_vm_context(&mut vm as *mut VM);

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
            vm.source_arg = "-".to_string();
            vm.user_args = remaining_args[i + 1..].to_vec();
            break;
        } else if arg == "--" {
            vm.user_args = remaining_args[i + 1..].to_vec();
            break;
        } else if files.is_empty() && eval_exprs.is_empty() {
            vm.source_arg = arg.clone();
            files.push(arg.clone());
            // Everything after the first file arg goes to user_args
            vm.user_args = remaining_args[i + 1..].to_vec();
            break;
        }
    }
    if eval_exprs.is_empty() && files.is_empty() && !read_stdin {
        // REPL mode: vm.source_arg stays "" and vm.user_args stays empty.
    } else if !eval_exprs.is_empty() && files.is_empty() && !read_stdin {
        vm.source_arg = "<eval>".to_string();
    }

    if read_stdin {
        if run_stdin(&mut vm, &mut symbols).is_err() {
            had_errors = true;
        }
    } else if !eval_exprs.is_empty() {
        for expr in &eval_exprs {
            if run_source(expr, "<eval>", &mut vm, &mut symbols).is_err() {
                had_errors = true;
            }
        }
    } else if !files.is_empty() {
        for filename in &files {
            if run_file(filename, &mut vm, &mut symbols).is_err() {
                had_errors = true;
            }
        }
    } else if run_repl(&mut vm, &mut symbols) {
        had_errors = true;
    }

    clear_vm_context();

    if elle::config::get().stats {
        let scope_stats = elle::lir::lower::global_scope_stats();
        if scope_stats.scopes_analyzed > 0 {
            eprint!("{}", scope_stats);
        }
        #[cfg(feature = "jit")]
        print_jit_stats(&mut vm);
        let cvc = elle::lir::closure_value_const_count();
        if cvc > 0 {
            eprintln!("[stats] closure-valued ValueConsts serialized: {}", cvc);
        }
    }

    if !read_stdin && files.is_empty() && eval_exprs.is_empty() {
        println!();
    }

    if had_errors {
        std::process::exit(1);
    }
}
