use elle::config::{self, Config};
use elle::context::{clear_vm_context, set_symbol_table, set_vm_context};
use elle::pipeline::compile_file;
use elle::repl::Repl;
use elle::{init_stdlib, register_primitives, SymbolTable, VM};
use serde_json::json;
use std::env;
use std::fs;
use std::io::{self, Read};

fn print_help() {
    println!("Elle v1.0.0\n");
    println!("Usage: elle [options] [file...] [-- args...]  Run files or start REPL");
    println!("       elle lint [options] <file|dir>...      Static analysis");
    println!("       elle lsp                              Start language server");
    println!("       elle rewrite [options] <file...>       Source-to-source rewriting\n");
    println!("Options:");
    println!("  -h, --help           Show this help");
    println!("  -e, --eval EXPR      Evaluate expression");
    println!("  -                    Read from stdin\n");
    println!("Execution:");
    println!("  --jit=N              JIT threshold (0=off, 1=first call, 11=default)");
    println!("  --wasm=N             WASM tiered compilation (0=off, N=threshold)");
    println!("  --wasm=full          Full-module WASM backend");
    println!("  --cache=PATH         Disk cache directory");
    println!("  --wasm-no-stdlib     Skip stdlib in full-module WASM mode");
    println!("  --no-uring           Disable io_uring");
    println!("  --json               JSON output on stderr (errors, stats, timing)");
    println!("  --stats              Print compilation stats on exit\n");
    println!("Paths:");
    println!("  --home=PATH          Elle home directory");
    println!("  --path=PATH          Module search path (colon-separated)\n");
    println!("Debug:");
    println!("  --debug              Print bytecode");
    println!("  --debug-jit          Print JIT decisions");
    println!("  --debug-resume       Print fiber resume traces");
    println!("  --debug-stack        Print stack operations");
    println!("  --debug-wasm         Print WASM host call traces");
    println!("  --wasm-dump          Dump WASM module to /tmp/elle-wasm-dump.wasm");
    println!("  --wasm-lir           Print LIR before WASM emission\n");
    println!("Use (help) in the REPL for primitives and special forms.");
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

/// Parse a "file:line:col: message" error string into components.
fn parse_error_location(error: &str) -> Option<(&str, &str)> {
    let colon_idx = error.find(": ")?;
    let location = &error[..colon_idx];
    if location.contains(':') {
        Some((location, &error[colon_idx + 2..]))
    } else {
        None
    }
}

/// Report a compilation error to stderr.
fn report_compilation_error(error: &str) {
    if config::get().json {
        let obj = if let Some((loc, msg)) = parse_error_location(error) {
            json!({"error": "compilation", "location": loc, "message": msg})
        } else {
            json!({"error": "compilation", "message": error})
        };
        eprintln!("{}", obj);
    } else if let Some((loc, msg)) = parse_error_location(error) {
        eprintln!("  at {}\n✗ Compilation error: {}", loc, msg);
    } else {
        eprintln!("✗ Compilation error: {}", error);
    }
}

/// Report a runtime error to stderr.
fn report_runtime_error(error: &str, symbols: &SymbolTable) {
    let resolved = format_runtime_error(error, symbols);
    if config::get().json {
        eprintln!("{}", json!({"error": "runtime", "message": resolved}));
    } else {
        eprintln!("{}", resolved);
    }
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
    if config::get().wasm_full {
        let eval_fn = if config::get().wasm_no_stdlib {
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
            report_compilation_error(&e);
            return Err(e);
        }
    };

    // Debug: print bytecode
    if config::get().debug {
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
            report_runtime_error(&e, symbols);
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

fn print_stats(vm: &VM) {
    let jit_compiled = vm.jit_cache.len();
    let jit_rejected = vm.jit_rejections.len();
    let total_calls: usize = vm.closure_call_counts.values().sum();
    let unique_closures = vm.closure_call_counts.len();
    let heap_objects = vm.fiber.heap.len();
    let heap_bytes = vm.fiber.heap.allocated_bytes();
    let plugins = vm.loaded_plugins.len();

    let mut jit_rejections_list: Vec<_> = vm.jit_rejections.iter().collect();
    jit_rejections_list
        .sort_by_key(|(ptr, _)| vm.closure_call_counts.get(ptr).copied().unwrap_or(0));

    if config::get().json {
        let rejections: Vec<_> = jit_rejections_list
            .iter()
            .map(|(ptr, info)| {
                let name = info.name.as_deref().unwrap_or("<anon>");
                let calls = vm.closure_call_counts.get(ptr).copied().unwrap_or(0);
                json!({"name": name, "reason": info.reason.to_string(), "calls": calls})
            })
            .collect();
        let obj = json!({
            "stats": {
                "jit": {
                    "compiled": jit_compiled,
                    "rejected": jit_rejected,
                    "rejections": rejections,
                },
                "calls": {
                    "total": total_calls,
                    "unique": unique_closures,
                },
                "memory": {
                    "heap_objects": heap_objects,
                    "heap_bytes": heap_bytes,
                },
                "plugins": plugins,
            }
        });
        eprintln!("{}", obj);
    } else {
        eprintln!("Stats:");
        eprintln!("  jit:");
        eprintln!("    compiled: {}", jit_compiled);
        eprintln!("    rejected: {}", jit_rejected);
        for (ptr, info) in &jit_rejections_list {
            let name = info.name.as_deref().unwrap_or("<anon>");
            let calls = vm.closure_call_counts.get(ptr).copied().unwrap_or(0);
            eprintln!("    {:<24} {}  [called {}x]", name, info.reason, calls);
        }
        eprintln!("  calls:");
        eprintln!("    total: {}", total_calls);
        eprintln!("    unique: {}", unique_closures);
        eprintln!("  memory:");
        eprintln!("    heap_objects: {}", heap_objects);
        eprintln!("    heap_bytes: {}", heap_bytes);
        eprintln!("  plugins: {}", plugins);
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

    // Parse CLI arguments into config + remaining positional args
    let cli_args: Vec<String> = args[1..].to_vec();

    // Check for --help/-h first (before VM init)
    if cli_args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return;
    }

    let (parsed_config, remaining) = match Config::parse(&cli_args) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("elle: {}", e);
            std::process::exit(1);
        }
    };
    config::init(parsed_config);

    // Interpreter mode — needs VM setup
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();

    let _signals = register_primitives(&mut vm, &mut symbols);

    set_symbol_table(&mut symbols as *mut SymbolTable);

    init_stdlib(&mut vm, &mut symbols);

    set_vm_context(&mut vm as *mut VM);

    let mut had_errors = false;
    let mut read_stdin = false;
    let mut has_source = false;

    // Process remaining args: find source files, eval expressions, user args
    // First non-flag arg is the source. Everything after it (or after --) is user args.
    let mut i = 0;
    while i < remaining.len() {
        let arg = &remaining[i];

        if arg == "--" {
            // Everything after -- goes to user args
            vm.user_args = remaining[i + 1..].to_vec();
            break;
        }

        if let Some(expr) = arg.strip_prefix("--eval:") {
            has_source = true;
            if run_source(expr, "<eval>", &mut vm, &mut symbols).is_err() {
                had_errors = true;
            }
            i += 1;
            continue;
        }

        if arg == "-" {
            read_stdin = true;
            has_source = true;
            vm.source_arg = "-".to_string();
            vm.user_args = remaining[i + 1..].to_vec();
            break;
        }

        // First positional arg is the source file
        vm.source_arg = arg.clone();
        vm.user_args = remaining[i + 1..].to_vec();
        has_source = true;
        if run_file(arg, &mut vm, &mut symbols).is_err() {
            had_errors = true;
        }
        break;
    }

    if read_stdin {
        if run_stdin(&mut vm, &mut symbols).is_err() {
            had_errors = true;
        }
    } else if !has_source && run_repl(&mut vm, &mut symbols) {
        had_errors = true;
    }

    clear_vm_context();

    if config::get().stats {
        print_stats(&vm);
    }

    if !has_source {
        println!();
    }

    if had_errors {
        std::process::exit(1);
    }
}
