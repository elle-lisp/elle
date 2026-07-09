//! The categorized [`ErrorKind`] enum.
//!
//! Split from the module root so the large variant list lives apart from the
//! `LError` wrapper, its rendering, and its builder methods.

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
