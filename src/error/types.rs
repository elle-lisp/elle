//! Error type definitions for Elle

use std::error::Error as StdError;
use std::fmt;

/// Comprehensive typed error enum for Elle
///
/// Replaces generic `Result<T, String>` errors with specific error types
/// for better error handling and context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EllError {
    // Type-related errors
    TypeMismatch {
        expected: String,
        got: String,
    },
    UndefinedVariable {
        name: String,
    },

    // Argument-related errors
    ArityMismatch {
        expected: usize,
        got: usize,
    },
    ArgumentError {
        message: String,
    },

    // Index-related errors
    IndexOutOfBounds {
        index: isize,
        length: usize,
    },

    // Arithmetic errors
    DivisionByZero,
    InvalidNumericOperation {
        operation: String,
        reason: String,
    },

    // FFI-related errors
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

    // Compiler errors
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

    // Runtime errors
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
    ExceptionInFinally {
        message: String,
    },

    // File/IO errors
    FileNotFound {
        path: String,
    },
    FileReadError {
        path: String,
        reason: String,
    },

    // Generic error for fallback
    Generic {
        message: String,
    },
}

impl fmt::Display for EllError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.description())
    }
}

impl StdError for EllError {}

/// Conversion from EllError to String for compatibility
impl From<EllError> for String {
    fn from(err: EllError) -> String {
        err.description()
    }
}

/// Conversion from String to EllError for fallback
impl From<String> for EllError {
    fn from(msg: String) -> Self {
        EllError::Generic { message: msg }
    }
}

impl From<&str> for EllError {
    fn from(msg: &str) -> Self {
        EllError::Generic {
            message: msg.to_string(),
        }
    }
}

impl EllError {
    /// Get a human-readable description of the error
    pub fn description(&self) -> String {
        match self {
            EllError::TypeMismatch { expected, got } => {
                format!("Type error: expected {}, got {}", expected, got)
            }
            EllError::UndefinedVariable { name } => {
                format!("Reference error: undefined variable '{}'", name)
            }
            EllError::ArityMismatch { expected, got } => {
                format!(
                    "Argument error: expected {} argument{}, got {}",
                    expected,
                    if *expected == 1 { "" } else { "s" },
                    got
                )
            }
            EllError::IndexOutOfBounds { index, length } => {
                format!(
                    "Index error: index {} out of bounds for length {}",
                    index, length
                )
            }
            EllError::DivisionByZero => "Arithmetic error: division by zero".to_string(),
            EllError::InvalidNumericOperation { operation, reason } => {
                format!("Arithmetic error in {}: {}", operation, reason)
            }
            EllError::FFIError { operation, message } => {
                format!("FFI error in {}: {}", operation, message)
            }
            EllError::LibraryNotFound { path } => {
                format!("Library not found: {}", path)
            }
            EllError::SymbolNotFound { library, symbol } => {
                format!("Symbol '{}' not found in library '{}'", symbol, library)
            }
            EllError::FFITypeError { ctype, message } => {
                format!("FFI type error for {}: {}", ctype, message)
            }
            EllError::SyntaxError { message, line } => match line {
                Some(l) => format!("Syntax error at line {}: {}", l, message),
                None => format!("Syntax error: {}", message),
            },
            EllError::CompileError { message } => format!("Compile error: {}", message),
            EllError::MacroError { message } => format!("Macro error: {}", message),
            EllError::PatternError { message } => format!("Pattern error: {}", message),
            EllError::RuntimeError { message } => format!("Runtime error: {}", message),
            EllError::ExecutionError { message } => format!("Execution error: {}", message),
            EllError::UncaughtException { message } => {
                format!("Uncaught exception: {}", message)
            }
            EllError::ExceptionInFinally { message } => {
                format!("Exception in finally clause: {}", message)
            }
            EllError::FileNotFound { path } => format!("File not found: {}", path),
            EllError::FileReadError { path, reason } => {
                format!("Failed to read file {}: {}", path, reason)
            }
            EllError::ArgumentError { message } => format!("Argument error: {}", message),
            EllError::Generic { message } => format!("Error: {}", message),
        }
    }
}
