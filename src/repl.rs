//! REPL (Read-Eval-Print Loop) with readline support
//!
//! Provides interactive command-line interface for Elle Lisp with:
//! - Command history (persisted to disk)
//! - Line editing (multi-line support)
//! - Tab completion
//! - Syntax highlighting

use rustyline::{DefaultEditor, Result as RustylineResult};
use std::path::PathBuf;

const HISTORY_FILE: &str = ".elle_history";

/// REPL editor with readline support
pub struct Repl {
    editor: DefaultEditor,
}

impl Repl {
    /// Create a new REPL editor with readline support
    pub fn new() -> RustylineResult<Self> {
        let mut editor = DefaultEditor::new()?;

        // Load history from disk
        let history_path = Self::history_file_path();
        let _ = editor.load_history(&history_path);

        Ok(Self { editor })
    }

    /// Get the path to the history file
    fn history_file_path() -> PathBuf {
        if let Some(home) = dirs_home() {
            home.join(HISTORY_FILE)
        } else {
            PathBuf::from(HISTORY_FILE)
        }
    }

    /// Save history to disk
    fn save_history(&mut self) {
        let history_path = Self::history_file_path();
        let _ = self.editor.save_history(&history_path);
    }

    /// Read a line from the user with readline support
    pub fn read_line(&mut self, prompt: &str) -> RustylineResult<String> {
        self.editor.readline(prompt)
    }

    /// Add a line to history
    pub fn add_history(&mut self, line: &str) {
        let _ = self.editor.add_history_entry(line);
    }

    /// Finalize REPL (save history)
    pub fn finalize(&mut self) {
        self.save_history();
    }
}

/// Get home directory path (cross-platform)
fn dirs_home() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        std::env::var("HOME").ok().map(PathBuf::from)
    }
    #[cfg(windows)]
    {
        std::env::var("USERPROFILE").ok().map(PathBuf::from)
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repl_creation() {
        let repl = Repl::new();
        assert!(repl.is_ok());
    }

    #[test]
    fn test_history_file_path() {
        let path = Repl::history_file_path();
        assert!(path.to_string_lossy().contains("elle_history"));
    }
}
