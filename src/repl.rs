// audited: 2026-09-20
//! The interactive prompt: accumulate lines until they make a form, evaluate
//! it, print what it answered.
//!
//! docs/pipeline.md
//!
//! Two loops read a session — rustyline's, and a bare-stdin fallback for a
//! terminal rustyline cannot open — and both do the same work per line. What
//! the prompt does past reading a line lives beside this file: splitting the
//! input into forms (`read`), running one and keeping what it binds (`eval`),
//! and holding a form whose references do not resolve yet (`defer`).

use crate::pipeline::CompileCtx;
use crate::symbol::SymbolTable;
use crate::vm::VM;

use rustyline::error::ReadlineError;
use rustyline::{DefaultEditor, Result as RustylineResult};

mod defer;
mod eval;
mod read;

use defer::{report_unresolved, DeferredForm};
use eval::try_eval_accumulated;

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
    pub fn run(&mut self, vm: &mut VM, symbols: &mut SymbolTable, cctx: &mut CompileCtx) -> bool {
        greet();
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

                    had_errors |= self.try_eval(vm, symbols, cctx);
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
    pub fn run_fallback(vm: &mut VM, symbols: &mut SymbolTable, cctx: &mut CompileCtx) -> bool {
        use std::io::{self, BufRead, Write};

        greet();
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

            had_errors |= try_eval_accumulated(&mut accumulated, vm, symbols, cctx, &mut deferred);
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
    fn try_eval(&mut self, vm: &mut VM, symbols: &mut SymbolTable, cctx: &mut CompileCtx) -> bool {
        try_eval_accumulated(&mut self.accumulated, vm, symbols, cctx, &mut self.deferred)
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

/// The greeting shared by the rustyline and fallback REPLs.
fn greet() {
    println!("{} (type (help) for commands)", crate::BANNER);
}

fn print_repl_help() {
    println!("{}\n", crate::BANNER);
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
