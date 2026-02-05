//! Comprehensive error typing system for Elle
//!
//! Replaces generic `Result<T, String>` with typed error enums for better
//! error handling, reporting, and composability.

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

impl EllError {
    /// Create a type mismatch error
    pub fn type_mismatch(expected: impl Into<String>, got: impl Into<String>) -> Self {
        EllError::TypeMismatch {
            expected: expected.into(),
            got: got.into(),
        }
    }

    /// Create an undefined variable error
    pub fn undefined_variable(name: impl Into<String>) -> Self {
        EllError::UndefinedVariable { name: name.into() }
    }

    /// Create an arity mismatch error
    pub fn arity_mismatch(expected: usize, got: usize) -> Self {
        EllError::ArityMismatch { expected, got }
    }

    /// Create an index out of bounds error
    pub fn index_out_of_bounds(index: isize, length: usize) -> Self {
        EllError::IndexOutOfBounds { index, length }
    }

    /// Create an FFI error
    pub fn ffi_error(operation: impl Into<String>, message: impl Into<String>) -> Self {
        EllError::FFIError {
            operation: operation.into(),
            message: message.into(),
        }
    }

    /// Create a library not found error
    pub fn library_not_found(path: impl Into<String>) -> Self {
        EllError::LibraryNotFound { path: path.into() }
    }

    /// Create a symbol not found error
    pub fn symbol_not_found(library: impl Into<String>, symbol: impl Into<String>) -> Self {
        EllError::SymbolNotFound {
            library: library.into(),
            symbol: symbol.into(),
        }
    }

    /// Create a syntax error
    pub fn syntax_error(message: impl Into<String>, line: Option<usize>) -> Self {
        EllError::SyntaxError {
            message: message.into(),
            line,
        }
    }

    /// Create a compile error
    pub fn compile_error(message: impl Into<String>) -> Self {
        EllError::CompileError {
            message: message.into(),
        }
    }

    /// Create a macro error
    pub fn macro_error(message: impl Into<String>) -> Self {
        EllError::MacroError {
            message: message.into(),
        }
    }

    /// Create a file not found error
    pub fn file_not_found(path: impl Into<String>) -> Self {
        EllError::FileNotFound { path: path.into() }
    }

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

/// Source code location (line and column)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceLoc {
    pub line: usize,
    pub col: usize,
}

impl fmt::Display for SourceLoc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

impl SourceLoc {
    /// Create a new source location
    pub fn new(line: usize, col: usize) -> Self {
        SourceLoc { line, col }
    }

    /// Create a location at the beginning of a file
    pub fn start() -> Self {
        SourceLoc { line: 1, col: 1 }
    }
}

/// Runtime error with optional source location
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeError {
    pub message: String,
    pub location: Option<SourceLoc>,
    pub context: Option<String>,
}

impl RuntimeError {
    /// Create a new runtime error
    pub fn new(message: String) -> Self {
        RuntimeError {
            message,
            location: None,
            context: None,
        }
    }

    /// Add location information
    pub fn with_location(mut self, location: SourceLoc) -> Self {
        self.location = Some(location);
        self
    }

    /// Add context information
    pub fn with_context(mut self, context: String) -> Self {
        self.context = Some(context);
        self
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.location {
            Some(loc) => write!(f, "Error at {}: {}", loc, self.message)?,
            None => write!(f, "Error: {}", self.message)?,
        }

        if let Some(ref ctx) = self.context {
            write!(f, "\n  Context: {}", ctx)?;
        }

        Ok(())
    }
}

impl std::error::Error for RuntimeError {}

/// Type mismatch error
pub fn type_mismatch(expected: &str, got: &str) -> String {
    format!("Type error: expected {}, got {}", expected, got)
}

/// Arity mismatch error
pub fn arity_mismatch(expected: usize, got: usize) -> String {
    format!(
        "Argument error: expected {} argument{}, got {}",
        expected,
        if expected == 1 { "" } else { "s" },
        got
    )
}

/// Index out of bounds error
pub fn index_out_of_bounds(index: isize, len: usize) -> String {
    format!(
        "Index error: index {} out of bounds for length {}",
        index, len
    )
}

/// Undefined variable error
pub fn undefined_variable(name: &str) -> String {
    format!("Reference error: undefined variable '{}'", name)
}

/// Division by zero error
pub fn division_by_zero() -> String {
    "Arithmetic error: division by zero".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_mismatch_error() {
        let err = EllError::type_mismatch("int", "string");
        assert_eq!(err.description(), "Type error: expected int, got string");
    }

    #[test]
    fn test_undefined_variable_error() {
        let err = EllError::undefined_variable("foo");
        assert_eq!(
            err.description(),
            "Reference error: undefined variable 'foo'"
        );
    }

    #[test]
    fn test_arity_mismatch_error_singular() {
        let err = EllError::arity_mismatch(1, 2);
        assert_eq!(
            err.description(),
            "Argument error: expected 1 argument, got 2"
        );
    }

    #[test]
    fn test_arity_mismatch_error_plural() {
        let err = EllError::arity_mismatch(2, 1);
        assert_eq!(
            err.description(),
            "Argument error: expected 2 arguments, got 1"
        );
    }

    #[test]
    fn test_index_out_of_bounds_error() {
        let err = EllError::index_out_of_bounds(10, 5);
        assert_eq!(
            err.description(),
            "Index error: index 10 out of bounds for length 5"
        );
    }

    #[test]
    fn test_division_by_zero_error() {
        let err = EllError::DivisionByZero;
        assert_eq!(err.description(), "Arithmetic error: division by zero");
    }

    #[test]
    fn test_ffi_error() {
        let err = EllError::ffi_error("load_library", "file not found");
        assert_eq!(
            err.description(),
            "FFI error in load_library: file not found"
        );
    }

    #[test]
    fn test_library_not_found_error() {
        let err = EllError::library_not_found("/lib/libc.so.6");
        assert_eq!(err.description(), "Library not found: /lib/libc.so.6");
    }

    #[test]
    fn test_symbol_not_found_error() {
        let err = EllError::symbol_not_found("libc", "strlen");
        assert_eq!(
            err.description(),
            "Symbol 'strlen' not found in library 'libc'"
        );
    }

    #[test]
    fn test_syntax_error_with_line() {
        let err = EllError::syntax_error("unexpected token", Some(42));
        assert_eq!(
            err.description(),
            "Syntax error at line 42: unexpected token"
        );
    }

    #[test]
    fn test_syntax_error_without_line() {
        let err = EllError::syntax_error("unexpected token", None);
        assert_eq!(err.description(), "Syntax error: unexpected token");
    }

    #[test]
    fn test_compile_error() {
        let err = EllError::compile_error("invalid expression");
        assert_eq!(err.description(), "Compile error: invalid expression");
    }

    #[test]
    fn test_macro_error() {
        let err = EllError::macro_error("macro expansion failed");
        assert_eq!(err.description(), "Macro error: macro expansion failed");
    }

    #[test]
    fn test_file_not_found_error() {
        let err = EllError::file_not_found("script.l");
        assert_eq!(err.description(), "File not found: script.l");
    }

    #[test]
    fn test_error_display_trait() {
        let err = EllError::undefined_variable("x");
        let display = format!("{}", err);
        assert_eq!(display, "Reference error: undefined variable 'x'");
    }

    #[test]
    fn test_error_to_string_conversion() {
        let err = EllError::type_mismatch("int", "bool");
        let s: String = err.into();
        assert_eq!(s, "Type error: expected int, got bool");
    }

    #[test]
    fn test_string_to_error_conversion() {
        let err: EllError = "some error message".to_string().into();
        assert_eq!(err.description(), "Error: some error message");
    }

    #[test]
    fn test_str_to_error_conversion() {
        let err: EllError = "some error".into();
        assert_eq!(err.description(), "Error: some error");
    }

    #[test]
    fn test_error_equality() {
        let err1 = EllError::type_mismatch("int", "string");
        let err2 = EllError::type_mismatch("int", "string");
        assert_eq!(err1, err2);
    }

    #[test]
    fn test_error_inequality() {
        let err1 = EllError::type_mismatch("int", "string");
        let err2 = EllError::type_mismatch("int", "bool");
        assert_ne!(err1, err2);
    }

    #[test]
    fn test_error_debug_format() {
        let err = EllError::DivisionByZero;
        let debug = format!("{:?}", err);
        assert!(debug.contains("DivisionByZero"));
    }

    #[test]
    fn test_ffi_type_error() {
        let err = EllError::FFITypeError {
            ctype: "struct Point".to_string(),
            message: "invalid field offset".to_string(),
        };
        assert_eq!(
            err.description(),
            "FFI type error for struct Point: invalid field offset"
        );
    }

    #[test]
    fn test_invalid_numeric_operation() {
        let err = EllError::InvalidNumericOperation {
            operation: "sqrt".to_string(),
            reason: "negative number".to_string(),
        };
        assert_eq!(
            err.description(),
            "Arithmetic error in sqrt: negative number"
        );
    }

    #[test]
    fn test_pattern_error() {
        let err = EllError::PatternError {
            message: "unreachable pattern".to_string(),
        };
        assert_eq!(err.description(), "Pattern error: unreachable pattern");
    }

    #[test]
    fn test_uncaught_exception() {
        let err = EllError::UncaughtException {
            message: "user exception".to_string(),
        };
        assert_eq!(err.description(), "Uncaught exception: user exception");
    }

    #[test]
    fn test_exception_in_finally() {
        let err = EllError::ExceptionInFinally {
            message: "cleanup failed".to_string(),
        };
        assert_eq!(
            err.description(),
            "Exception in finally clause: cleanup failed"
        );
    }

    #[test]
    fn test_source_loc_creation() {
        let loc = SourceLoc::new(10, 5);
        assert_eq!(loc.line, 10);
        assert_eq!(loc.col, 5);
    }

    #[test]
    fn test_source_loc_start() {
        let loc = SourceLoc::start();
        assert_eq!(loc.line, 1);
        assert_eq!(loc.col, 1);
    }

    #[test]
    fn test_source_loc_display() {
        let loc = SourceLoc::new(42, 13);
        let display = format!("{}", loc);
        assert_eq!(display, "42:13");
    }

    #[test]
    fn test_runtime_error_with_location() {
        let err = RuntimeError::new("test error".to_string()).with_location(SourceLoc::new(5, 10));
        assert!(err.location.is_some());
        assert_eq!(err.location.unwrap().line, 5);
    }

    #[test]
    fn test_runtime_error_with_context() {
        let err =
            RuntimeError::new("test error".to_string()).with_context("in function foo".to_string());
        assert!(err.context.is_some());
        assert_eq!(err.context.as_ref().unwrap(), "in function foo");
    }

    #[test]
    fn test_runtime_error_display_with_location() {
        let err = RuntimeError::new("test error".to_string()).with_location(SourceLoc::new(42, 5));
        let display = format!("{}", err);
        assert!(display.contains("42:5"));
        assert!(display.contains("test error"));
    }

    #[test]
    fn test_runtime_error_display_with_context() {
        let err = RuntimeError::new("test error".to_string()).with_context("in main".to_string());
        let display = format!("{}", err);
        assert!(display.contains("in main"));
    }

    #[test]
    fn test_error_as_std_error() {
        let err: Box<dyn StdError> = Box::new(EllError::DivisionByZero);
        assert_eq!(err.to_string(), "Arithmetic error: division by zero");
    }

    #[test]
    fn test_multiple_error_types_different() {
        let err1 = EllError::type_mismatch("int", "string");
        let err2 = EllError::undefined_variable("x");
        assert_ne!(format!("{:?}", err1), format!("{:?}", err2));
    }

    #[test]
    fn test_argument_error() {
        let err = EllError::ArgumentError {
            message: "invalid format string".to_string(),
        };
        assert_eq!(err.description(), "Argument error: invalid format string");
    }

    #[test]
    fn test_execution_error() {
        let err = EllError::ExecutionError {
            message: "infinite loop detected".to_string(),
        };
        assert_eq!(err.description(), "Execution error: infinite loop detected");
    }

    #[test]
    fn test_file_read_error() {
        let err = EllError::FileReadError {
            path: "file.l".to_string(),
            reason: "permission denied".to_string(),
        };
        assert_eq!(
            err.description(),
            "Failed to read file file.l: permission denied"
        );
    }

    #[test]
    fn test_generic_error() {
        let err = EllError::Generic {
            message: "something went wrong".to_string(),
        };
        assert_eq!(err.description(), "Error: something went wrong");
    }
}
