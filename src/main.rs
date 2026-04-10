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
    println!("       elle lint [options] <file|dir>... Static analysis");
    println!("       elle lsp                          Start language server");
    println!("       elle rewrite [options] <file...>  Source-to-source rewriting\n");
    println!("Options:");
    println!("  -h, --help        Show this help");
    println!("  -                 Read from stdin");
    println!("  --jit=N           JIT threshold (0=off, 1=immediate, default: 11)");
    println!("  --wasm=N|full     WASM backend (0=off, N=tiered, full=whole-module)");
    println!("  --stats           Print compilation stats on exit");
    println!("  --json            JSON output on stderr\n");
    println!("Environment:");
    println!("  ELLE_HOME             Module resolution root");
    println!("  ELLE_PATH             Colon-separated module search path");
    println!("  ELLE_CACHE            Disk cache directory\n");
    print!("{}", elle::primitives::help_text());
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

/// Parse a compilation error string and format with location on separate line.
/// Format: "file:line:col: message" -> "  at file:line:col\n✗ Compilation error: message"
fn format_compilation_error(error: &str) -> String {
    // Try to extract location and message
    // Pattern: "file:line:col: message"
    if let Some(colon_idx) = error.find(": ") {
        let location_part = &error[..colon_idx];
        // Check if this looks like a location (contains at least one colon for line:col)
        if location_part.contains(':') {
            let message = &error[colon_idx + 2..];
            return format!("  at {}\n✗ Compilation error: {}", location_part, message);
        }
    }
    // Fallback: just show error as-is
    format!("✗ Compilation error: {}", error)
}

fn run_stdin(vm: &mut VM, symbols: &mut SymbolTable) -> Result<(), String> {
    let mut contents = String::new();
    io::stdin()
        .read_to_string(&mut contents)
        .map_err(|e| format!("Failed to read stdin: {}", e))?;

    run_source(&contents, "<stdin>", vm, symbols)
}

fn run_file(filename: &str, vm: &mut VM, symbols: &mut SymbolTable) -> Result<(), String> {
    let mut contents =
        fs::read_to_string(filename).map_err(|e| format!("Failed to read file: {}", e))?;

    // Strip shebang if present (e.g., #!/usr/bin/env elle)
    if contents.starts_with("#!") {
        contents = contents.lines().skip(1).collect::<Vec<_>>().join("\n");
    }

    run_source(&contents, filename, vm, symbols)
}

/// Run Elle source code from a string.
/// Only prints non-nil results.
fn run_source(
    contents: &str,
    source_name: &str,
    vm: &mut VM,
    symbols: &mut SymbolTable,
) -> Result<(), String> {
    // WASM backend: compile and run through Wasmtime instead of bytecode VM
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
            eprintln!("{}", format_compilation_error(&e));
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

fn print_jit_stats(vm: &VM) {
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
    // Ensure enough static TLS space for dlopen'd plugins.
    // GLIBC_TUNABLES is read by the dynamic linker before main(), so if
    // it's missing we must re-exec ourselves with it set.
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;

        let key = "GLIBC_TUNABLES";
        let needed = "glibc.rtld.optional_static_tls=65536";
        let already_set = env::var(key)
            .map(|v| v.contains("glibc.rtld.optional_static_tls"))
            .unwrap_or(false);
        if !already_set {
            // Merge with any existing tunables
            let new_val = match env::var(key) {
                Ok(existing) if !existing.is_empty() => format!("{}:{}", existing, needed),
                _ => needed.to_string(),
            };
            let exe = env::current_exe().expect("failed to get current exe path");
            let err = std::process::Command::new(exe)
                .args(&env::args().collect::<Vec<_>>()[1..])
                .env(key, &new_val)
                .exec();
            eprintln!("elle: re-exec failed: {}", err);
            std::process::exit(1);
        }
    }

    let args: Vec<String> = env::args().collect();

    // Subcommand dispatch — no VM setup needed for these
    match args.get(1).map(|s| s.as_str()) {
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
    let mut read_stdin = false;

    // remaining_args from Config::parse: file args, eval expressions, and user args after --.
    if let Some(first) = remaining_args.first() {
        if first == "-" {
            read_stdin = true;
            vm.source_arg = "-".to_string();
            vm.user_args = remaining_args[1..].to_vec();
        } else {
            vm.source_arg = first.clone();
            vm.user_args = remaining_args[1..].to_vec();
            files.push(first.clone());
        }
    }
    // If no source arg found: REPL mode, vm.source_arg stays "" and vm.user_args stays empty.

    if read_stdin {
        if let Err(e) = run_stdin(&mut vm, &mut symbols) {
            eprintln!("Error: {}", e);
            had_errors = true;
        }
    } else if !files.is_empty() {
        for filename in &files {
            if let Err(e) = run_file(filename, &mut vm, &mut symbols) {
                eprintln!("Error: {}", e);
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
        print_jit_stats(&vm);
        let cvc = elle::lir::closure_value_const_count();
        if cvc > 0 {
            eprintln!("[stats] closure-valued ValueConsts serialized: {}", cvc);
        }
    }

    if !read_stdin && files.is_empty() {
        println!();
    }

    if had_errors {
        std::process::exit(1);
    }
}
