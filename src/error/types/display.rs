//! Rendering for [`LError`]: `description`, source-context formatting, the
//! `Display`/`Error` trait impls, and the `String`/`&str` conversions.

use super::{ErrorKind, LError, TraceSource};
use std::error::Error as StdError;
use std::fmt;

impl LError {
    /// Get a human-readable description
    pub fn description(&self) -> String {
        match &self.kind {
            ErrorKind::TypeMismatch { expected, got } => {
                format!("Type error: expected {}, got {}", expected, got)
            }
            ErrorKind::UndefinedVariable { name, suggestions } => {
                if suggestions.is_empty() {
                    format!("undefined variable: {}", name)
                } else {
                    format!(
                        "undefined variable: {} (did you mean: {}?)",
                        name,
                        suggestions.join(", ")
                    )
                }
            }
            ErrorKind::ArityMismatch { expected, got } => {
                format!(
                    "Argument error: expected {} argument{}, got {}",
                    expected,
                    if *expected == 1 { "" } else { "s" },
                    got
                )
            }
            ErrorKind::ArityAtLeast { minimum, got } => {
                format!(
                    "Argument error: expected at least {} argument{}, got {}",
                    minimum,
                    if *minimum == 1 { "" } else { "s" },
                    got
                )
            }
            ErrorKind::ArityRange { min, max, got } => {
                format!(
                    "Argument error: expected {}-{} arguments, got {}",
                    min, max, got
                )
            }
            ErrorKind::IndexOutOfBounds { index, length } => {
                format!(
                    "Index error: index {} out of bounds for length {}",
                    index, length
                )
            }
            ErrorKind::DivisionByZero => "Arithmetic error: division by zero".to_string(),
            ErrorKind::NumericOverflow { operation } => {
                format!("Arithmetic error: overflow in {}", operation)
            }
            ErrorKind::InvalidNumericOperation { operation, reason } => {
                format!("Arithmetic error in {}: {}", operation, reason)
            }
            ErrorKind::FFIError { operation, message } => {
                format!("FFI error in {}: {}", operation, message)
            }
            ErrorKind::LibraryNotFound { path } => {
                format!("Library not found: {}", path)
            }
            ErrorKind::SymbolNotFound { library, symbol } => {
                format!("Symbol '{}' not found in library '{}'", symbol, library)
            }
            ErrorKind::FFITypeError { ctype, message } => {
                format!("FFI type error for {}: {}", ctype, message)
            }
            ErrorKind::SyntaxError { message, line } => match line {
                Some(l) => format!("Syntax error at line {}: {}", l, message),
                None => format!("Syntax error: {}", message),
            },
            ErrorKind::CompileError { message } => format!("Compile error: {}", message),
            ErrorKind::MacroError { message } => format!("Macro error: {}", message),
            ErrorKind::PatternError { message } => format!("Pattern error: {}", message),
            ErrorKind::SignalMismatch {
                function,
                required_mask,
                actual_mask,
            } => {
                format!(
                    "function {} restricted to {} but body may emit {}",
                    function, required_mask, actual_mask
                )
            }
            ErrorKind::UnterminatedForm { delimiter, depth } => {
                let closer = match delimiter {
                    '(' => "paren",
                    '[' => "bracket",
                    '{' => "brace",
                    '|' => "pipe",
                    _ => "delimiter",
                };
                if *depth > 1 {
                    format!(
                        "unterminated {} (missing {} closing {}s)",
                        delimiter, depth, closer
                    )
                } else {
                    format!("unterminated {} (missing closing {})", delimiter, closer)
                }
            }
            ErrorKind::RuntimeError { message } => format!("Runtime error: {}", message),
            ErrorKind::ExecutionError { message } => format!("Execution error: {}", message),
            ErrorKind::UncaughtException { message } => {
                format!("Uncaught exception: {}", message)
            }
            ErrorKind::FileNotFound { path } => format!("File not found: {}", path),
            ErrorKind::FileReadError { path, reason } => {
                format!("Failed to read file {}: {}", path, reason)
            }
            ErrorKind::ArgumentError { message } => format!("Argument error: {}", message),
            ErrorKind::Generic { message } => format!("Error: {}", message),
        }
    }
}

impl LError {
    /// Format this error with source context (carets).
    ///
    /// When the location points to a readable file, shows the source
    /// line with a `^` caret. This is the rich display used by the CLI;
    /// the `Display` impl is a simpler fallback that doesn't do I/O.
    pub fn format_with_source(&self) -> String {
        let mut out = String::new();
        if let Some(ref loc) = self.location {
            out.push_str(&format!("  at {}\n", loc));
            if let Some(source) = crate::error::formatting::load_source_for_loc(loc) {
                let ctx = crate::error::formatting::format_source_context(&source, loc);
                if !ctx.is_empty() {
                    out.push_str(&ctx);
                }
            }
        }
        out.push_str(&format!("✗ {}", self.description()));
        match &self.trace {
            TraceSource::None => {}
            TraceSource::Vm(frames) | TraceSource::Cps(frames) => {
                for frame in frames {
                    out.push_str("\n    in ");
                    if let Some(ref name) = frame.function_name {
                        out.push_str(name);
                    } else {
                        out.push_str("<anonymous>");
                    }
                    if let Some(ref loc) = frame.location {
                        out.push_str(&format!(" at {}", loc));
                    }
                }
            }
        }
        out
    }
}

impl fmt::Display for LError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ref loc) = self.location {
            writeln!(f, "  at {}", loc)?;
        }
        write!(f, "✗ {}", self.description())?;
        match &self.trace {
            TraceSource::None => {}
            TraceSource::Vm(frames) | TraceSource::Cps(frames) => {
                for frame in frames {
                    write!(f, "\n    in ")?;
                    if let Some(ref name) = frame.function_name {
                        write!(f, "{}", name)?;
                    } else {
                        write!(f, "<anonymous>")?;
                    }
                    if let Some(ref loc) = frame.location {
                        write!(f, " at {}", loc)?;
                    }
                }
            }
        }
        Ok(())
    }
}

impl StdError for LError {}

// Compatibility conversions
impl From<LError> for String {
    fn from(err: LError) -> String {
        err.description()
    }
}

impl From<String> for LError {
    fn from(msg: String) -> Self {
        LError::new(ErrorKind::Generic { message: msg })
    }
}

impl From<&str> for LError {
    fn from(msg: &str) -> Self {
        LError::new(ErrorKind::Generic {
            message: msg.to_string(),
        })
    }
}
