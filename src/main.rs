use elle::context::{clear_vm_context, set_symbol_table, set_vm_context};
use elle::pipeline::{compile, compile_file};
use elle::primitives::set_length_symbol_table;
use elle::repl::Repl;
use elle::{init_stdlib, register_primitives, SymbolTable, VM};
use rustyline::error::ReadlineError;
use std::env;
use std::fs;
use std::io::{self, Read, Write};

fn print_welcome() {
    println!("Elle v1.0.0 (type (help) for commands)");
}

fn print_error_context(input: &str, _msg: &str, line: usize, col: usize) {
    let lines: Vec<&str> = input.lines().collect();

    if line > 0 && line <= lines.len() {
        let line_str = lines[line - 1];
        eprintln!("  {}", line_str);

        // Print caret pointing to error location
        if col > 0 {
            eprintln!("  {}^", " ".repeat(col - 1));
        }
    }
}

fn print_help() {
    println!("Elle v1.0.0\n");
    println!("Usage: elle [file...] [-- args...]       Run files or start REPL");
    println!("       elle lint [options] <file|dir>... Static analysis");
    println!("       elle lsp                          Start language server");
    println!("       elle rewrite [options] <file...>  Source-to-source rewriting\n");
    println!("Options:");
    println!("  -h, --help    Show this help");
    println!("  -             Read from stdin\n");
    println!("Environment:");
    println!("  ELLE_JIT_STATS=1  Print JIT compilation stats to stderr on exit\n");
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
    // Compile file as a single letrec
    let result = match compile_file(contents, symbols, source_name) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{}", format_compilation_error(&e));
            return Err(e);
        }
    };

    // Debug: print bytecode if ELLE_DEBUG is set
    if std::env::var("ELLE_DEBUG").is_ok() {
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
    print_welcome();

    // Create REPL with readline support
    let mut repl = match Repl::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("✗ Failed to initialize readline: {}", e);
            // Fall back to basic stdin reading
            return run_repl_fallback(vm, symbols);
        }
    };

    let mut accumulated_input = String::new();
    let mut had_errors = false;

    loop {
        // Read line with readline support
        match repl.read_line("> ") {
            Ok(input) => {
                let input = input.trim();
                if input.is_empty() {
                    continue;
                }

                accumulated_input.push_str(input);
                accumulated_input.push('\n');

                // Add to history
                repl.add_history(input);

                // Check for built-in REPL commands
                match input {
                    "(exit)" | "exit" => break,
                    "(help)" | "help" => {
                        print_help();
                        accumulated_input.clear();
                        continue;
                    }
                    _ => {}
                }

                // Try to compile accumulated input
                match compile(accumulated_input.trim(), symbols, "<repl>") {
                    Ok(result) => {
                        accumulated_input.clear();

                        // Execute
                        match vm.execute_scheduled(&result.bytecode, symbols) {
                            Ok(value) => {
                                if !value.is_nil() {
                                    println!("⟹ {:?}", value);
                                }
                            }
                            Err(e) => {
                                eprintln!("{}", format_runtime_error(&e, symbols));
                                had_errors = true;
                            }
                        }
                    }
                    Err(e) => {
                        // Check if this is just an incomplete expression
                        let err_msg = e.to_string();
                        if err_msg.contains("Unterminated")
                            || err_msg.contains("unexpected end of input")
                        {
                            // Expression is incomplete, prompt for more input on next line
                            // Don't print an error, just continue accumulating
                        } else {
                            // Real parse error - extract line and column from error message
                            let err_msg = e.to_string();
                            eprintln!("{}", format_compilation_error(&err_msg));

                            // Try to extract line and column from format like "<input>:1:3: message"
                            let (line, col) = if let Some(colon_pos) = err_msg.find(':') {
                                let rest = &err_msg[colon_pos + 1..];
                                if let Ok(line_num) =
                                    rest.split(':').next().unwrap_or("1").parse::<usize>()
                                {
                                    if let Ok(col_num) =
                                        rest.split(':').nth(1).unwrap_or("1").parse::<usize>()
                                    {
                                        (line_num, col_num)
                                    } else {
                                        (line_num, 1)
                                    }
                                } else {
                                    (1, 1)
                                }
                            } else {
                                (1, 1)
                            };

                            print_error_context(
                                accumulated_input.trim(),
                                "compilation error",
                                line,
                                col,
                            );
                            accumulated_input.clear();
                            had_errors = true;
                        }
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("^C");
                accumulated_input.clear();
                continue;
            }
            Err(ReadlineError::Eof) => {
                break;
            }
            Err(e) => {
                eprintln!("✗ Readline error: {}", e);
                had_errors = true;
                break;
            }
        }
    }

    // Save history
    repl.finalize();

    had_errors
}

fn run_repl_fallback(vm: &mut VM, symbols: &mut SymbolTable) -> bool {
    eprintln!("Using fallback stdin input (no history or editing)");

    let mut accumulated_input = String::new();
    let mut had_errors = false;

    loop {
        print!("> ");
        io::stdout().flush().unwrap();

        let mut line = String::new();
        match io::stdin().read_line(&mut line) {
            Ok(0) => break, // EOF
            Err(_) => break,
            Ok(_) => {}
        }

        accumulated_input.push_str(&line);

        let trimmed = accumulated_input.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Check for built-in REPL commands
        match trimmed {
            "(exit)" | "exit" => break,
            "(help)" | "help" => {
                print_help();
                accumulated_input.clear();
                continue;
            }
            _ => {}
        }

        // Try to compile accumulated input
        match compile(trimmed, symbols, "<repl>") {
            Ok(result) => {
                accumulated_input.clear();

                // Execute
                match vm.execute_scheduled(&result.bytecode, symbols) {
                    Ok(value) => {
                        if !value.is_nil() {
                            println!("⟹ {:?}", value);
                        }
                    }
                    Err(e) => {
                        eprintln!("{}", format_runtime_error(&e, symbols));
                        had_errors = true;
                    }
                }
            }
            Err(e) => {
                // Check if this is just an incomplete expression
                let err_msg = e.to_string();
                if err_msg.contains("Unterminated") || err_msg.contains("unexpected end of input") {
                    // Expression is incomplete, prompt for more input
                    print!(". ");
                    io::stdout().flush().unwrap();
                } else {
                    // Real parse error - extract line and column from error message
                    let err_msg = e.to_string();
                    eprintln!("{}", format_compilation_error(&err_msg));

                    // Try to extract line and column from format like "<input>:1:3: message"
                    let (line, col) = if let Some(colon_pos) = err_msg.find(':') {
                        let rest = &err_msg[colon_pos + 1..];
                        if let Ok(line_num) = rest.split(':').next().unwrap_or("1").parse::<usize>()
                        {
                            if let Ok(col_num) =
                                rest.split(':').nth(1).unwrap_or("1").parse::<usize>()
                            {
                                (line_num, col_num)
                            } else {
                                (line_num, 1)
                            }
                        } else {
                            (1, 1)
                        }
                    } else {
                        (1, 1)
                    };

                    print_error_context(trimmed, "compilation error", line, col);
                    accumulated_input.clear();
                    had_errors = true;
                }
            }
        }
    }

    had_errors
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

    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();

    let _signals = register_primitives(&mut vm, &mut symbols);

    set_symbol_table(&mut symbols as *mut SymbolTable);
    set_length_symbol_table(&mut symbols as *mut SymbolTable);

    init_stdlib(&mut vm, &mut symbols);

    set_vm_context(&mut vm as *mut VM);

    let mut had_errors = false;
    let mut files: Vec<&str> = Vec::new();
    let mut read_stdin = false;

    // Find the first source arg: "-" for stdin, or any arg not starting with "-".
    // Everything after it becomes user_args for sys/args.
    // Args before it that start with "-" are silently ignored (unknown flags).
    let all_args = &args[1..];
    if let Some(idx) = all_args
        .iter()
        .position(|a| a == "-" || !a.starts_with('-'))
    {
        let source_arg = &all_args[idx];
        vm.user_args = all_args[idx + 1..].iter().cloned().collect();
        if source_arg == "-" {
            read_stdin = true;
        } else {
            files.push(source_arg.as_str());
        }
    }
    // If no source arg found: REPL mode, vm.user_args stays empty.

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

    if env::var_os("ELLE_JIT_STATS").is_some() {
        print_jit_stats(&vm);
    }

    if !read_stdin && files.is_empty() {
        println!();
    }

    if had_errors {
        std::process::exit(1);
    }
}
