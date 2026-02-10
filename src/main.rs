use elle::compiler::converters::value_to_expr;
use elle::ffi::primitives::context::set_symbol_table;
use elle::ffi_primitives;
use elle::repl::Repl;
use elle::{compile, init_stdlib, read_str, register_primitives, SymbolTable, VM};
use rustyline::error::ReadlineError;
use std::env;
use std::fs;
use std::io::{self, Write};

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
    println!("  Strings:     string-length, string-append, string-upcase, string-downcase,");
    println!("               substring, string-index, char-at");
    println!("  Vectors:     vector, vector-length, vector-ref, vector-set!");
    println!("  Types:       type, int, float, string");
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

fn run_file(filename: &str, vm: &mut VM, symbols: &mut SymbolTable) -> Result<(), String> {
    let mut contents =
        fs::read_to_string(filename).map_err(|e| format!("Failed to read file: {}", e))?;

    // Strip shebang if present (e.g., #!/usr/bin/env elle)
    if contents.starts_with("#!") {
        contents = contents.lines().skip(1).collect::<Vec<_>>().join("\n");
    }

    let mut had_parse_error = false;
    let mut had_runtime_error = false;
    let mut had_compilation_error = false;

    // First pass: collect all top-level definitions to pre-register them
    // This allows recursive functions to reference themselves
    {
        let mut lexer = elle::reader::Lexer::new(&contents);
        let mut temp_tokens = Vec::new();
        let mut temp_locations = Vec::new();
        loop {
            match lexer.next_token_with_loc() {
                Ok(Some(mut token_with_loc)) => {
                    // Set the file name in the location
                    token_with_loc.loc.file = filename.to_string();
                    temp_tokens.push(elle::reader::OwnedToken::from(token_with_loc.token));
                    temp_locations.push(token_with_loc.loc);
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }

        let mut temp_reader = elle::reader::Reader::with_locations(temp_tokens, temp_locations);
        while let Some(result) = temp_reader.try_read(symbols) {
            match result {
                Ok(value) => {
                    // Check if this is a define
                    if let Ok(list) = value.list_to_vec() {
                        if list.len() >= 3 {
                            if let elle::value::Value::Symbol(sym) = &list[0] {
                                let name = symbols.name(*sym).unwrap_or("");
                                if name == "define" {
                                    if let Ok(def_name) = list[1].as_symbol() {
                                        // Pre-register the symbol as nil so forward references work
                                        vm.set_global(def_name.0, elle::value::Value::Nil);
                                    }
                                }
                            }
                        }
                    }
                }
                Err(_) => {
                    // Suppress error reporting in first pass; errors will be reported in second pass
                }
            }
        }
    }

    // Second pass: execute all expressions
    let mut lexer = elle::reader::Lexer::new(&contents);
    let mut tokens = Vec::new();
    let mut locations = Vec::new();
    loop {
        match lexer.next_token_with_loc() {
            Ok(Some(mut token_with_loc)) => {
                // Set the file name in the location
                token_with_loc.loc.file = filename.to_string();
                tokens.push(elle::reader::OwnedToken::from(token_with_loc.token));
                locations.push(token_with_loc.loc);
            }
            Ok(None) => break,
            Err(e) => return Err(format!("Lexer error: {}", e)),
        }
    }

    let mut reader = elle::reader::Reader::with_locations(tokens, locations);
    while let Some(result) = reader.try_read(symbols) {
        match result {
            Ok(value) => {
                // Compile
                let expr = match value_to_expr(&value, symbols) {
                    Ok(e) => e,
                    Err(e) => {
                        eprintln!("✗ Compilation error: {}", e);
                        had_compilation_error = true;
                        continue;
                    }
                };

                let bytecode = compile(&expr);

                // Execute
                match vm.execute(&bytecode) {
                    Ok(result) => {
                        if !result.is_nil() {
                            println!("⟹ {:?}", result);
                        }
                    }
                    Err(e) => {
                        eprintln!("✗ Runtime error: {}", e);
                        had_runtime_error = true;
                    }
                }
            }
            Err(e) => {
                eprintln!("✗ Parse error: {}", e);
                had_parse_error = true;
            }
        }
    }

    // Return error if any errors occurred (will exit with status 1)
    if had_parse_error || had_runtime_error || had_compilation_error {
        Err("Errors encountered during execution".to_string())
    } else {
        Ok(())
    }
}

fn run_repl(vm: &mut VM, symbols: &mut SymbolTable) {
    print_welcome();

    // Create REPL with readline support
    let mut repl = match Repl::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("✗ Failed to initialize readline: {}", e);
            // Fall back to basic stdin reading
            run_repl_fallback(vm, symbols);
            return;
        }
    };

    let mut accumulated_input = String::new();

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

                // Try to parse accumulated input
                match read_str(accumulated_input.trim(), symbols) {
                    Ok(value) => {
                        accumulated_input.clear();

                        // Compile
                        let expr = match value_to_expr(&value, symbols) {
                            Ok(e) => e,
                            Err(e) => {
                                eprintln!("✗ Compilation error: {}", e);
                                continue;
                            }
                        };

                        let bytecode = compile(&expr);

                        // Execute
                        match vm.execute(&bytecode) {
                            Ok(result) => {
                                if !result.is_nil() {
                                    println!("⟹ {:?}", result);
                                }
                            }
                            Err(e) => {
                                eprintln!("✗ Runtime error: {}", e);
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
                            eprintln!("✗ Parse error: {}", err_msg);

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

                            print_error_context(accumulated_input.trim(), "parse error", line, col);
                            accumulated_input.clear();
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
                break;
            }
        }
    }

    // Save history
    repl.finalize();
}

fn run_repl_fallback(vm: &mut VM, symbols: &mut SymbolTable) {
    eprintln!("Using fallback stdin input (no history or editing)");

    let mut accumulated_input = String::new();

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

        // Try to parse accumulated input
        match read_str(trimmed, symbols) {
            Ok(value) => {
                accumulated_input.clear();

                // Compile
                let expr = match value_to_expr(&value, symbols) {
                    Ok(e) => e,
                    Err(e) => {
                        eprintln!("✗ Compilation error: {}", e);
                        continue;
                    }
                };

                let bytecode = compile(&expr);

                // Execute
                match vm.execute(&bytecode) {
                    Ok(result) => {
                        if !result.is_nil() {
                            println!("⟹ {:?}", result);
                        }
                    }
                    Err(e) => {
                        eprintln!("✗ Runtime error: {}", e);
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
                    eprintln!("✗ Parse error: {}", err_msg);

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

                    print_error_context(trimmed, "parse error", line, col);
                    accumulated_input.clear();
                }
            }
        }
    }
}

fn main() {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();

    // Register primitive functions
    register_primitives(&mut vm, &mut symbols);

    // Initialize standard library modules
    init_stdlib(&mut vm, &mut symbols);

    // Set VM context for FFI primitives
    ffi_primitives::set_vm_context(&mut vm as *mut VM);

    // Set symbol table context for primitives
    set_symbol_table(&mut symbols as *mut SymbolTable);

    // Check for command-line arguments
    let args: Vec<String> = env::args().collect();
    let mut had_errors = false;
    let mut use_jit = false;
    let mut files = Vec::new();

    // Parse flags and files
    for arg in &args[1..] {
        if arg == "--jit" {
            use_jit = true;
        } else if !arg.starts_with('-') {
            files.push(arg.as_str());
        }
    }

    if use_jit {
        eprintln!("Elle: JIT mode enabled (experimental)");
    }

    if !files.is_empty() {
        // Run file(s)
        for filename in files {
            if let Err(e) = run_file(filename, &mut vm, &mut symbols) {
                eprintln!("Error: {}", e);
                had_errors = true;
            }
        }
    } else if args.len() == 1 || (use_jit && args.len() == 2) {
        // Run REPL
        run_repl(&mut vm, &mut symbols);
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
