//! Error type definitions for Elle

use crate::reader::SourceLoc;
use std::error::Error as StdError;
use std::fmt;

/// Stack frame for error traces
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackFrame {
    pub function_name: Option<String>,
    pub location: Option<SourceLoc>,
}

/// Source of stack trace — supports deferred capture
#[derive(Debug, Clone, Default)]
pub enum TraceSource {
    /// No trace available
    #[default]
    None,
    /// Captured from bytecode VM
    Vm(Vec<StackFrame>),
    /// Captured from CPS continuation chain (future)
    Cps(Vec<StackFrame>),
}

/// Unified error type for Elle
#[derive(Debug, Clone)]
pub struct LError {
    pub kind: ErrorKind,
    pub location: Option<SourceLoc>,
    pub trace: TraceSource,
}

/// Categorized error kinds
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    // Type errors
    TypeMismatch {
        expected: String,
        got: String,
    },
    UndefinedVariable {
        name: String,
        suggestions: Vec<String>,
    },

    // Arity errors
    ArityMismatch {
        expected: usize,
        got: usize,
    },
    ArityAtLeast {
        minimum: usize,
        got: usize,
    },
    ArityRange {
        min: usize,
        max: usize,
        got: usize,
    },
    ArgumentError {
        message: String,
    },

    // Index errors
    IndexOutOfBounds {
        index: isize,
        length: usize,
    },

    // Arithmetic
    DivisionByZero,
    NumericOverflow {
        operation: String,
    },
    InvalidNumericOperation {
        operation: String,
        reason: String,
    },

    // FFI
    FFIError {
        operation: String,
        message: String,
    },
    LibraryNotFound {
        path: String,
    },
    SymbolNotFound {
        library: String,
        symbol: String,
    },
    FFITypeError {
        ctype: String,
        message: String,
    },

    // Compiler
    SyntaxError {
        message: String,
        line: Option<usize>,
    },
    CompileError {
        message: String,
    },
    MacroError {
        message: String,
    },
    PatternError {
        message: String,
    },
    SignalMismatch {
        function: String,
        required_mask: String,
        actual_mask: String,
    },
    UnterminatedForm {
        delimiter: char,
        depth: usize,
    },

    // Runtime
    RuntimeError {
        message: String,
    },
    ExecutionError {
        message: String,
    },

    // Exception handling
    UncaughtException {
        message: String,
    },

    // IO
    FileNotFound {
        path: String,
    },
    FileReadError {
        path: String,
        reason: String,
    },

    // Fallback
    Generic {
        message: String,
    },
}

/// Result type alias
pub type LResult<T> = Result<T, LError>;

impl LError {
    /// Create a new error with just a kind
    pub fn new(kind: ErrorKind) -> Self {
        LError {
            kind,
            location: None,
            trace: TraceSource::None,
        }
    }

    /// Add location information
    pub fn with_location(mut self, loc: SourceLoc) -> Self {
        self.location = Some(loc);
        self
    }

    /// Add trace information
    pub fn with_trace(mut self, trace: TraceSource) -> Self {
        self.trace = trace;
        self
    }

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

// Builder methods for constructing LError instances

impl LError {
    // Type errors
    pub fn type_mismatch(expected: impl Into<String>, got: impl Into<String>) -> Self {
        LError::new(ErrorKind::TypeMismatch {
            expected: expected.into(),
            got: got.into(),
        })
    }

    pub fn undefined_variable(name: impl Into<String>) -> Self {
        LError::new(ErrorKind::UndefinedVariable {
            name: name.into(),
            suggestions: Vec::new(),
        })
    }

    pub fn undefined_variable_with_suggestions(
        name: impl Into<String>,
        suggestions: Vec<String>,
    ) -> Self {
        LError::new(ErrorKind::UndefinedVariable {
            name: name.into(),
            suggestions,
        })
    }

    // Arity errors
    pub fn arity_mismatch(expected: usize, got: usize) -> Self {
        LError::new(ErrorKind::ArityMismatch { expected, got })
    }

    pub fn arity_at_least(minimum: usize, got: usize) -> Self {
        LError::new(ErrorKind::ArityAtLeast { minimum, got })
    }

    pub fn arity_range(min: usize, max: usize, got: usize) -> Self {
        LError::new(ErrorKind::ArityRange { min, max, got })
    }

    pub fn argument_error(message: impl Into<String>) -> Self {
        LError::new(ErrorKind::ArgumentError {
            message: message.into(),
        })
    }

    // Index errors
    pub fn index_out_of_bounds(index: isize, length: usize) -> Self {
        LError::new(ErrorKind::IndexOutOfBounds { index, length })
    }

    // Arithmetic
    pub fn division_by_zero() -> Self {
        LError::new(ErrorKind::DivisionByZero)
    }

    pub fn numeric_overflow(operation: impl Into<String>) -> Self {
        LError::new(ErrorKind::NumericOverflow {
            operation: operation.into(),
        })
    }

    pub fn invalid_numeric_operation(
        operation: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        LError::new(ErrorKind::InvalidNumericOperation {
            operation: operation.into(),
            reason: reason.into(),
        })
    }

    // FFI
    pub fn ffi_error(operation: impl Into<String>, message: impl Into<String>) -> Self {
        LError::new(ErrorKind::FFIError {
            operation: operation.into(),
            message: message.into(),
        })
    }

    pub fn library_not_found(path: impl Into<String>) -> Self {
        LError::new(ErrorKind::LibraryNotFound { path: path.into() })
    }

    pub fn symbol_not_found(library: impl Into<String>, symbol: impl Into<String>) -> Self {
        LError::new(ErrorKind::SymbolNotFound {
            library: library.into(),
            symbol: symbol.into(),
        })
    }

    pub fn ffi_type_error(ctype: impl Into<String>, message: impl Into<String>) -> Self {
        LError::new(ErrorKind::FFITypeError {
            ctype: ctype.into(),
            message: message.into(),
        })
    }

    // Compiler
    pub fn syntax_error(message: impl Into<String>, line: Option<usize>) -> Self {
        LError::new(ErrorKind::SyntaxError {
            message: message.into(),
            line,
        })
    }

    pub fn compile_error(message: impl Into<String>) -> Self {
        LError::new(ErrorKind::CompileError {
            message: message.into(),
        })
    }

    pub fn macro_error(message: impl Into<String>) -> Self {
        LError::new(ErrorKind::MacroError {
            message: message.into(),
        })
    }

    pub fn pattern_error(message: impl Into<String>) -> Self {
        LError::new(ErrorKind::PatternError {
            message: message.into(),
        })
    }

    pub fn signal_mismatch(
        function: impl Into<String>,
        required_mask: impl Into<String>,
        actual_mask: impl Into<String>,
    ) -> Self {
        LError::new(ErrorKind::SignalMismatch {
            function: function.into(),
            required_mask: required_mask.into(),
            actual_mask: actual_mask.into(),
        })
    }

    pub fn unterminated_form(delimiter: char, depth: usize) -> Self {
        LError::new(ErrorKind::UnterminatedForm { delimiter, depth })
    }

    // Runtime
    pub fn runtime_error(message: impl Into<String>) -> Self {
        LError::new(ErrorKind::RuntimeError {
            message: message.into(),
        })
    }

    pub fn execution_error(message: impl Into<String>) -> Self {
        LError::new(ErrorKind::ExecutionError {
            message: message.into(),
        })
    }

    // Exception handling
    pub fn uncaught_exception(message: impl Into<String>) -> Self {
        LError::new(ErrorKind::UncaughtException {
            message: message.into(),
        })
    }

    // IO
    pub fn file_not_found(path: impl Into<String>) -> Self {
        LError::new(ErrorKind::FileNotFound { path: path.into() })
    }

    pub fn file_read_error(path: impl Into<String>, reason: impl Into<String>) -> Self {
        LError::new(ErrorKind::FileReadError {
            path: path.into(),
            reason: reason.into(),
        })
    }

    // Generic
    pub fn generic(message: impl Into<String>) -> Self {
        LError::new(ErrorKind::Generic {
            message: message.into(),
        })
    }
}
