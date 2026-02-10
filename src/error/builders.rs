//! Helper functions for constructing EllError instances

use super::types::EllError;

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
}

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
