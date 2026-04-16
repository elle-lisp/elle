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
    println!("  -h, --help            Show this help");
    println!("  -e, --eval EXPR       Evaluate expression");
    println!("  -                     Read from stdin");
    println!("  --dump-ast            Print parsed AST as s-expressions and exit");
    println!("  --jit=POLICY          JIT policy: off, eager, adaptive (default), or integer N");
    println!("  --wasm=POLICY         WASM policy: off (default), full, lazy, or integer N");
    println!(
        "  --trace=KW[,KW,...]   Trace subsystems: call, signal, fiber, jit, wasm, compile, ..."
    );
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

/// Run Elle source code from a string.
/// Only prints non-nil results.
fn run_source(
    contents: &str,
    source_name: &str,
    vm: &mut VM,
    symbols: &mut SymbolTable,
) -> Result<(), String> {
    // --dump-ast: parse and print, then exit without compiling
    if elle::config::get().dump_ast {
        let forms = elle::reader::read_syntax_all_for(contents, source_name).map_err(|e| {
            eprintln!("{}", e);
            e
        })?;
        for form in &forms {
            println!("{}", form);
        }
        return Ok(());
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
        print_jit_stats(&vm);
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
