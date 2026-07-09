//! Builder constructors for [`LError`], one per [`ErrorKind`] category.

use super::{ErrorKind, LError};

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
