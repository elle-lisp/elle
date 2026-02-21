use elle::ffi::primitives::context::set_symbol_table;
use elle::ffi_primitives;
use elle::pipeline::{compile_all_new, compile_new};
use elle::primitives::set_length_symbol_table;
use elle::repl::Repl;
use elle::{init_stdlib, register_primitives, SymbolTable, VM};
use rustyline::error::ReadlineError;
use std::env;
use std::fs;
use std::io::{self, Read, Write};

fn print_welcome() {
    println!("Elle v0.1.0 - Lisp Interpreter (type (help) for commands)");
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
    println!();
    println!("Elle - Fast Lisp Interpreter");
    println!();
    println!("Primitives:");
    println!("  Arithmetic:  +, -, *, /");
    println!("  Comparison:  =, <, >, <=, >=");
    println!("  Lists:       cons, first, rest, list, length, append, reverse");
    println!("  List utils:  nth, last, take, drop");
    println!("  Math:        min, max, abs, sqrt, sin, cos, tan, log, exp, pow");
    println!("  Constants:   pi, e");
    println!("  Rounding:    floor, ceil, round");
    println!("  Integer ops: mod, remainder, even?, odd?");
    println!("  Strings:     string-append, string-upcase, string-downcase,");
    println!("               substring, string-index, char-at");
    println!("  Vectors:     vector, vector-ref, vector-set!");
    println!("  Types:       type-of, int, float, string");
    println!("  Logic:       not, if");
    println!("  I/O:         display, newline");
    println!();
    println!("Special forms:");
    println!("  (if cond then else)  - Conditional");
    println!("  (quote x)            - Quote literal");
    println!("  (define x 10)        - Define variable");
    println!("  (begin ...)          - Sequence");
    println!();
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
    _source_name: &str,
    vm: &mut VM,
    symbols: &mut SymbolTable,
) -> Result<(), String> {
    let mut had_error = false;

    // Compile all forms with new pipeline
    let results = match compile_all_new(contents, symbols) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("✗ Compilation error: {}", e);
            return Err(e);
        }
    };

    // Execute each compiled form
    for result in results {
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

        match vm.execute(&result.bytecode) {
            Ok(_) => {
                // Script mode is silent except for explicit output (display, etc.)
            }
            Err(e) => {
                eprintln!("✗ Runtime error: {}", format_runtime_error(&e, symbols));
                had_error = true;
            }
        }
    }

    // Return error if any errors occurred (will exit with status 1)
    if had_error {
        Err("Errors encountered during execution".to_string())
    } else {
        Ok(())
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
                match compile_new(accumulated_input.trim(), symbols) {
                    Ok(result) => {
                        accumulated_input.clear();

                        // Execute
                        match vm.execute(&result.bytecode) {
                            Ok(value) => {
                                if !value.is_nil() {
                                    println!("⟹ {:?}", value);
                                }
                            }
                            Err(e) => {
                                eprintln!("✗ Runtime error: {}", format_runtime_error(&e, symbols));
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
                            eprintln!("✗ Compilation error: {}", err_msg);

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
        match compile_new(trimmed, symbols) {
            Ok(result) => {
                accumulated_input.clear();

                // Execute
                match vm.execute(&result.bytecode) {
                    Ok(value) => {
                        if !value.is_nil() {
                            println!("⟹ {:?}", value);
                        }
                    }
                    Err(e) => {
                        eprintln!("✗ Runtime error: {}", format_runtime_error(&e, symbols));
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
                    eprintln!("✗ Compilation error: {}", err_msg);

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

fn main() {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();

    // Register primitive functions (effects map not needed in main)
    let _effects = register_primitives(&mut vm, &mut symbols);

    // Initialize standard library modules
    init_stdlib(&mut vm, &mut symbols);

    // Set VM context for FFI primitives
    ffi_primitives::set_vm_context(&mut vm as *mut VM);

    // Set symbol table context for primitives
    set_symbol_table(&mut symbols as *mut SymbolTable);

    // Set symbol table context for length primitive
    set_length_symbol_table(&mut symbols as *mut SymbolTable);

    // Check for command-line arguments
    let args: Vec<String> = env::args().collect();
    let mut had_errors = false;
    let mut files = Vec::new();
    let mut read_stdin = false;

    // Parse flags and files
    for arg in &args[1..] {
        if arg == "-" {
            // `-` means read from stdin
            read_stdin = true;
        } else if !arg.starts_with('-') {
            files.push(arg.as_str());
        }
    }

    if read_stdin {
        // Read from stdin (piped input)
        if let Err(e) = run_stdin(&mut vm, &mut symbols) {
            eprintln!("Error: {}", e);
            had_errors = true;
        }
    } else if !files.is_empty() {
        // Run file(s)
        for filename in files {
            if let Err(e) = run_file(filename, &mut vm, &mut symbols) {
                eprintln!("Error: {}", e);
                had_errors = true;
            }
        }
    } else if args.len() == 1 {
        // Run REPL
        if run_repl(&mut vm, &mut symbols) {
            had_errors = true;
        }
    }

    // Clear VM context
    ffi_primitives::clear_vm_context();

    if args.len() == 1 {
        println!();
    }

    // Exit with appropriate status code
    if had_errors {
        std::process::exit(1);
    }
}
