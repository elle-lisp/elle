//! REPL (Read-Eval-Print Loop)
//!
//! Compiles and executes forms one at a time. Each `def` extends the
//! compilation environment for subsequent inputs via the compilation
//! cache (same mechanism as stdlib). Multi-line accumulation detects
//! incomplete input by checking for "unterminated" reader errors.
//!
//! Forward references and mutual recursion are supported via deferred
//! compilation: when a def/defn form fails due to undefined variables,
//! it is saved and retried after subsequent definitions arrive.
//! Mutually recursive definitions are batch-compiled as a single
//! letrec unit.

use crate::pipeline::{compile_file_repl, register_repl_binding, register_repl_macros};
use crate::reader::read_syntax_all;
use crate::signals::Signal;
use crate::symbol::SymbolTable;
use crate::syntax::{Syntax, SyntaxKind};
use crate::value::types::Arity;
use crate::value::Value;
use crate::vm::VM;

use rustyline::error::ReadlineError;
use rustyline::{DefaultEditor, Result as RustylineResult};

const HISTORY_FILE: &str = ".elle_history";

// ── Public interface ─────────────────────────────────────────────────

/// A REPL session: readline state + input accumulation.
pub struct Repl {
    editor: DefaultEditor,
    accumulated: String,
    deferred: Vec<DeferredForm>,
}

impl Repl {
    pub fn new() -> RustylineResult<Self> {
        let mut editor = DefaultEditor::new()?;
        let _ = editor.load_history(&Self::history_path());
        Ok(Self {
            editor,
            accumulated: String::new(),
            deferred: Vec::new(),
        })
    }

    /// Run the interactive REPL loop. Returns true if any errors occurred.
    pub fn run(&mut self, vm: &mut VM, symbols: &mut SymbolTable) -> bool {
        println!("Elle v1.0.0 (type (help) for commands)");
        let mut had_errors = false;

        loop {
            let prompt = if self.accumulated.is_empty() {
                "> "
            } else {
                ". "
            };

            match self.editor.readline(prompt) {
                Ok(line) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let _ = self.editor.add_history_entry(trimmed);

                    if self.accumulated.is_empty() {
                        match trimmed {
                            "(exit)" | "exit" => break,
                            "(help)" | "help" => {
                                print_repl_help();
                                continue;
                            }
                            _ => {}
                        }
                    }

                    self.accumulated.push_str(&line);
                    self.accumulated.push('\n');

                    had_errors |= self.try_eval(vm, symbols);
                }
                Err(ReadlineError::Interrupted) => {
                    println!("^C");
                    self.accumulated.clear();
                }
                Err(ReadlineError::Eof) => break,
                Err(e) => {
                    eprintln!("✗ Readline error: {}", e);
                    had_errors = true;
                    break;
                }
            }
        }

        if !self.accumulated.trim().is_empty() {
            eprintln!("✗ <repl>: unterminated input at end of stream");
            had_errors = true;
        }

        had_errors |= report_unresolved(&self.deferred);

        let _ = self.editor.save_history(&Self::history_path());
        had_errors
    }

    /// Run the REPL with basic stdin (no readline).
    pub fn run_fallback(vm: &mut VM, symbols: &mut SymbolTable) -> bool {
        use std::io::{self, BufRead, Write};

        println!("Elle v1.0.0 (type (help) for commands)");
        let mut accumulated = String::new();
        let mut deferred: Vec<DeferredForm> = Vec::new();
        let mut had_errors = false;
        let stdin = io::stdin();

        loop {
            let prompt = if accumulated.is_empty() { "> " } else { ". " };
            print!("{}", prompt);
            let _ = io::stdout().flush();

            let mut line = String::new();
            match stdin.lock().read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if accumulated.is_empty() {
                match trimmed {
                    "(exit)" | "exit" => break,
                    "(help)" | "help" => {
                        print_repl_help();
                        continue;
                    }
                    _ => {}
                }
            }

            accumulated.push_str(&line);

            had_errors |= try_eval_accumulated(&mut accumulated, vm, symbols, &mut deferred);
        }

        if !accumulated.trim().is_empty() {
            eprintln!("✗ <repl>: unterminated input at end of stream");
            had_errors = true;
        }

        had_errors |= report_unresolved(&deferred);

        had_errors
    }

    // ── Private ──────────────────────────────────────────────────────

    fn history_path() -> String {
        match dirs_home() {
            Some(home) => crate::path::join(&[&home, HISTORY_FILE]),
            None => HISTORY_FILE.to_string(),
        }
    }

    /// Try to parse and evaluate accumulated input.
    /// Returns true if an error occurred.
    fn try_eval(&mut self, vm: &mut VM, symbols: &mut SymbolTable) -> bool {
        try_eval_accumulated(&mut self.accumulated, vm, symbols, &mut self.deferred)
    }
}

// ── Core evaluation logic (shared by readline and fallback) ──────────

/// Try to parse and evaluate accumulated input.
/// Clears `accumulated` on success or hard error. Leaves it intact on
/// incomplete input. Returns true if an error occurred.
fn try_eval_accumulated(
    accumulated: &mut String,
    vm: &mut VM,
    symbols: &mut SymbolTable,
    deferred: &mut Vec<DeferredForm>,
) -> bool {
    let mut had_errors = false;

    match try_read(accumulated) {
        ReadResult::Complete(forms) => {
            accumulated.clear();
            for form in &forms {
                match eval_form(form, vm, symbols) {
                    Ok(value) => {
                        if !value.is_nil() {
                            println!("⟹ {:?}", value);
                        }
                    }
                    Err(e) => {
                        if let Some(d) = try_defer(form, &e) {
                            eprintln!("{}: deferred ({} undefined)", d.name, d.missing.join(", "));
                            deferred.push(d);
                        } else {
                            eprintln!("✗ {}", e);
                            had_errors = true;
                        }
                    }
                }
                try_resolve_deferred(deferred, vm, symbols);
            }
        }
        ReadResult::Incomplete => {}
        ReadResult::Error(e) => {
            eprintln!("✗ {}", e);
            accumulated.clear();
            had_errors = true;
        }
    }

    had_errors
}

// ── Deferred compilation ─────────────────────────────────────────────

/// A def/defn form whose compilation was deferred due to undefined
/// variable references. Retried after subsequent definitions arrive.
struct DeferredForm {
    source: String,
    name: String,
    missing: Vec<String>,
}

/// Check whether a compilation error is a deferrable undefined-variable
/// error on a def/defn form. Only simple (non-destructuring) defs are
/// deferred.
fn try_defer(form: &FormInfo, error: &str) -> Option<DeferredForm> {
    if form.bindings.len() != 1 {
        return None;
    }
    if !error.contains("undefined variable:") {
        return None;
    }
    let undefined_vars = extract_undefined_vars(error);
    if undefined_vars.is_empty() {
        return None;
    }
    Some(DeferredForm {
        source: form.source.clone(),
        name: form.bindings[0].name.clone(),
        missing: undefined_vars,
    })
}

/// Extract undefined variable names from a compilation error message.
fn extract_undefined_vars(error: &str) -> Vec<String> {
    let mut vars = Vec::new();
    for line in error.lines() {
        if let Some(idx) = line.find("undefined variable: ") {
            let rest = &line[idx + "undefined variable: ".len()..];
            let name: String = rest
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != '(')
                .collect();
            if !name.is_empty() {
                vars.push(name);
            }
        }
    }
    vars
}

/// Try to resolve deferred forms. Two phases:
///
/// 1. **Individual**: recompile each deferred form alone. If its
///    missing references are now in the compilation cache, it
///    compiles and gets registered. Repeat until no progress.
///
/// 2. **Batch**: compile all remaining deferred forms together as a
///    single letrec. This handles mutual recursion: the letrec
///    pre-binds all names, allowing them to reference each other.
fn try_resolve_deferred(deferred: &mut Vec<DeferredForm>, vm: &mut VM, symbols: &mut SymbolTable) {
    if deferred.is_empty() {
        return;
    }

    // Phase 1: individual resolution (fixpoint loop)
    let mut changed = true;
    while changed {
        changed = false;
        let mut i = 0;
        while i < deferred.len() {
            if try_resolve_single(&deferred[i], vm, symbols) {
                eprintln!("{}: resolved", deferred[i].name);
                deferred.remove(i);
                changed = true;
            } else {
                i += 1;
            }
        }
    }

    // Phase 2: batch resolution for mutual recursion
    if deferred.len() >= 2 && try_batch_resolve(deferred, vm, symbols) {
        // Batch resolved some forms; try individual again
        // (resolving a batch may unblock other deferred forms).
        try_resolve_deferred(deferred, vm, symbols);
    }
}

/// Try to compile and register a single deferred form.
fn try_resolve_single(form: &DeferredForm, vm: &mut VM, symbols: &mut SymbolTable) -> bool {
    let Ok((result, expander)) = compile_file_repl(&form.source, symbols, "<repl>") else {
        return false;
    };
    register_repl_macros(expander.macros());
    let Ok(value) = vm.execute_scheduled(&result.bytecode, symbols) else {
        return false;
    };
    let sym_id = symbols.intern(&form.name);
    let (signal, arity) = extract_signal_arity(&value);
    register_repl_binding(sym_id, value, signal, arity);
    true
}

/// Batch-compile all deferred forms as a single letrec. The letrec
/// pre-binds every name, enabling mutual recursion among the group.
/// A trailing tuple expression extracts each binding's value.
fn try_batch_resolve(
    deferred: &mut Vec<DeferredForm>,
    vm: &mut VM,
    symbols: &mut SymbolTable,
) -> bool {
    let mut combined = String::new();
    let mut all_names: Vec<String> = Vec::new();

    for form in deferred.iter() {
        combined.push_str(&form.source);
        combined.push('\n');
        all_names.push(form.name.clone());
    }

    // Trailing tuple: [name1 name2 ...]
    combined.push_str(&format!("[{}]", all_names.join(" ")));

    let Ok((result, expander)) = compile_file_repl(&combined, symbols, "<repl>") else {
        return false;
    };
    register_repl_macros(expander.macros());
    let Ok(tuple_val) = vm.execute_scheduled(&result.bytecode, symbols) else {
        return false;
    };

    if let Some(items) = tuple_val.as_array() {
        for (form, val) in deferred.iter().zip(items.iter()) {
            let sym_id = symbols.intern(&form.name);
            let (signal, arity) = extract_signal_arity(val);
            register_repl_binding(sym_id, *val, signal, arity);
        }
        let names: Vec<&str> = all_names.iter().map(|s| s.as_str()).collect();
        eprintln!("{}: resolved", names.join(", "));
        deferred.clear();
        true
    } else {
        false
    }
}

/// Report unresolved deferred forms at session end. Returns true if
/// any exist (indicating an error).
fn report_unresolved(deferred: &[DeferredForm]) -> bool {
    for form in deferred {
        eprintln!(
            "✗ {}: unresolved ({} undefined)",
            form.name,
            form.missing.join(", ")
        );
    }
    !deferred.is_empty()
}

// ── Reading ──────────────────────────────────────────────────────────

/// Result of attempting to parse accumulated input.
enum ReadResult {
    /// Input parsed into one or more complete forms.
    Complete(Vec<FormInfo>),
    /// Input is incomplete (unterminated delimiter).
    Incomplete,
    /// Hard parse error.
    Error(String),
}

/// A parsed form with enough metadata to compile it individually.
struct FormInfo {
    /// Source text of this form (sliced from accumulated input via span byte offsets).
    source: String,
    /// Bindings introduced by this form, if any.
    bindings: Vec<DefBinding>,
}

/// A binding introduced by a top-level def/var form.
struct DefBinding {
    name: String,
}

/// Try to parse source into complete forms.
fn try_read(source: &str) -> ReadResult {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return ReadResult::Incomplete;
    }

    match read_syntax_all(trimmed, "<repl>") {
        Ok(syntaxes) if syntaxes.is_empty() => ReadResult::Incomplete,
        Ok(syntaxes) => {
            let forms = syntaxes
                .iter()
                .map(|syn| FormInfo {
                    source: trimmed[syn.span.start..syn.span.end].to_string(),
                    bindings: extract_def_bindings(syn),
                })
                .collect();
            ReadResult::Complete(forms)
        }
        Err(e) if is_incomplete_error(&e) => ReadResult::Incomplete,
        Err(e) => ReadResult::Error(e),
    }
}

/// Check whether a reader error indicates incomplete input.
fn is_incomplete_error(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("unterminated") || lower.contains("unexpected end of input")
}

/// Extract binding names from a def/var/defn form.
///
/// Handles:
/// - `(def name ...)` / `(var name ...)` / `(defn name ...)` → one name
/// - `(def [a b] ...)` / `(var [a b] ...)` → leaf names from destructure
/// - `(def {x y} ...)` → leaf names from struct destructure
fn extract_def_bindings(syntax: &Syntax) -> Vec<DefBinding> {
    if let SyntaxKind::List(items) = &syntax.kind {
        if items.len() >= 2 {
            if let Some(head) = items[0].as_symbol() {
                match head {
                    "def" | "var" => {
                        if let Some(name) = items[1].as_symbol() {
                            return vec![DefBinding {
                                name: name.to_string(),
                            }];
                        }
                        // Destructuring pattern
                        let mut names = Vec::new();
                        collect_pattern_names(&items[1], &mut names);
                        return names;
                    }
                    "defn" => {
                        if let Some(name) = items[1].as_symbol() {
                            return vec![DefBinding {
                                name: name.to_string(),
                            }];
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    Vec::new()
}

/// Recursively collect leaf symbol names from a destructuring pattern.
fn collect_pattern_names(syntax: &Syntax, out: &mut Vec<DefBinding>) {
    match &syntax.kind {
        SyntaxKind::Symbol(s) if s != "_" => {
            out.push(DefBinding {
                name: s.to_string(),
            });
        }
        SyntaxKind::Array(items) | SyntaxKind::List(items) | SyntaxKind::Struct(items) => {
            for item in items {
                collect_pattern_names(item, out);
            }
        }
        _ => {}
    }
}

// ── Evaluation ───────────────────────────────────────────────────────

/// Compile and execute a single form. If it introduces bindings
/// (def/var/defn, including destructuring), register each in the
/// compilation cache so subsequent forms see them.
fn eval_form(form: &FormInfo, vm: &mut VM, symbols: &mut SymbolTable) -> Result<Value, String> {
    if form.bindings.len() <= 1 {
        // No bindings or simple def: compile the form as-is.
        // For simple def, the letrec body is the bound name, so the
        // return value IS the bound value.
        let (result, expander) = compile_file_repl(&form.source, symbols, "<repl>")?;
        register_repl_macros(expander.macros());
        let value = vm.execute_scheduled(&result.bytecode, symbols)?;

        if let Some(binding) = form.bindings.first() {
            let sym_id = symbols.intern(&binding.name);
            let (signal, arity) = extract_signal_arity(&value);
            register_repl_binding(sym_id, value, signal, arity);
        }

        Ok(value)
    } else {
        // Destructuring def: compile the def followed by a tuple of
        // all leaf names. compile_file wraps both in a letrec, so the
        // tuple expression can reference the destructured bindings.
        // The return value is the tuple; we unpack it to register each.
        let names: Vec<&str> = form.bindings.iter().map(|b| b.name.as_str()).collect();
        let trailer = format!("[{}]", names.join(" "));
        let combined = format!("{} {}", form.source, trailer);

        let (result, expander) = compile_file_repl(&combined, symbols, "<repl>")?;
        register_repl_macros(expander.macros());
        let tuple_val = vm.execute_scheduled(&result.bytecode, symbols)?;

        // Register each leaf binding from the tuple.
        if let Some(items) = tuple_val.as_array() {
            for (binding, val) in form.bindings.iter().zip(items.iter()) {
                let sym_id = symbols.intern(&binding.name);
                let (signal, arity) = extract_signal_arity(val);
                register_repl_binding(sym_id, *val, signal, arity);
            }
        }

        Ok(tuple_val)
    }
}

/// Extract signal and arity from a runtime value.
fn extract_signal_arity(value: &Value) -> (Signal, Option<Arity>) {
    match value.as_closure() {
        Some(closure) => (closure.effective_signal(), Some(closure.template.arity)),
        None => (Signal::silent(), None),
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

fn print_repl_help() {
    println!("Elle v1.0.0\n");
    println!("REPL commands:");
    println!("  (exit)   Exit the REPL");
    println!("  (help)   Show this help");
    println!("  Ctrl-C   Cancel current input");
    println!("  Ctrl-D   Exit the REPL\n");
    print!("{}", crate::primitives::help_text());
}

fn dirs_home() -> Option<String> {
    #[cfg(unix)]
    {
        std::env::var("HOME").ok()
    }
    #[cfg(windows)]
    {
        std::env::var("USERPROFILE").ok()
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}
