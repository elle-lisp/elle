//! Comprehensive error typing system for Elle
//!
//! Replaces generic `Result<T, String>` with typed error enums for better
//! error handling, reporting, and composability.

use std::collections::HashMap;

mod builders;
pub mod formatting;
mod runtime;
mod sourceloc;
mod types;

// Re-export public API
pub use builders::{
    arity_mismatch, division_by_zero, index_out_of_bounds, type_mismatch, undefined_variable,
};
pub use runtime::RuntimeError;
pub use sourceloc::SourceLoc;
pub use types::EllError;

/// Mapping from bytecode instruction index to source code location
///
/// Used for generating runtime error messages with source location information.
/// Maps instruction pointers to the source location they originated from.
/// Uses SourceLoc from the reader module which includes file information.
pub type LocationMap = HashMap<usize, SourceLoc>;

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
        let err = EllError::file_not_found("script.lisp");
        assert_eq!(err.description(), "File not found: script.lisp");
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
        let loc = SourceLoc::from_line_col(10, 5);
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
        let loc = SourceLoc::from_line_col(42, 13);
        let display = format!("{}", loc);
        assert!(display.contains("42:13"));
    }

    #[test]
    fn test_runtime_error_with_location() {
        let err = RuntimeError::new("test error".to_string())
            .with_location(SourceLoc::from_line_col(5, 10));
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
        let err = RuntimeError::new("test error".to_string())
            .with_location(SourceLoc::from_line_col(42, 5));
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
        use std::error::Error as StdError;
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
            path: "file.lisp".to_string(),
            reason: "permission denied".to_string(),
        };
        assert_eq!(
            err.description(),
            "Failed to read file file.lisp: permission denied"
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
