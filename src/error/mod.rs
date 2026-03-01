//! Unified error system for Elle

mod builders;
pub mod formatting;
mod runtime;
mod types;

use std::collections::HashMap;

// Re-export core types
pub use crate::reader::SourceLoc;
pub use types::{ErrorKind, LError, LResult, StackFrame, TraceSource};

// Keep RuntimeError for now (can deprecate later)
pub use runtime::RuntimeError;

/// Mapping from bytecode instruction index to source location
pub type LocationMap = HashMap<usize, SourceLoc>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_mismatch_error() {
        let err = LError::type_mismatch("int", "string");
        assert_eq!(err.description(), "Type error: expected int, got string");
    }

    #[test]
    fn test_undefined_variable_error() {
        let err = LError::undefined_variable("foo");
        assert_eq!(
            err.description(),
            "Reference error: undefined variable 'foo'"
        );
    }

    #[test]
    fn test_arity_mismatch_error_singular() {
        let err = LError::arity_mismatch(1, 2);
        assert_eq!(
            err.description(),
            "Argument error: expected 1 argument, got 2"
        );
    }

    #[test]
    fn test_arity_mismatch_error_plural() {
        let err = LError::arity_mismatch(2, 1);
        assert_eq!(
            err.description(),
            "Argument error: expected 2 arguments, got 1"
        );
    }

    #[test]
    fn test_arity_at_least_error() {
        let err = LError::arity_at_least(2, 1);
        assert_eq!(
            err.description(),
            "Argument error: expected at least 2 arguments, got 1"
        );
    }

    #[test]
    fn test_arity_range_error() {
        let err = LError::arity_range(2, 4, 1);
        assert_eq!(
            err.description(),
            "Argument error: expected 2-4 arguments, got 1"
        );
    }

    #[test]
    fn test_index_out_of_bounds_error() {
        let err = LError::index_out_of_bounds(10, 5);
        assert_eq!(
            err.description(),
            "Index error: index 10 out of bounds for length 5"
        );
    }

    #[test]
    fn test_division_by_zero_error() {
        let err = LError::division_by_zero();
        assert_eq!(err.description(), "Arithmetic error: division by zero");
    }

    #[test]
    fn test_ffi_error() {
        let err = LError::ffi_error("load_library", "file not found");
        assert_eq!(
            err.description(),
            "FFI error in load_library: file not found"
        );
    }

    #[test]
    fn test_library_not_found_error() {
        let err = LError::library_not_found("/lib/libc.so.6");
        assert_eq!(err.description(), "Library not found: /lib/libc.so.6");
    }

    #[test]
    fn test_symbol_not_found_error() {
        let err = LError::symbol_not_found("libc", "strlen");
        assert_eq!(
            err.description(),
            "Symbol 'strlen' not found in library 'libc'"
        );
    }

    #[test]
    fn test_syntax_error_with_line() {
        let err = LError::syntax_error("unexpected token", Some(42));
        assert_eq!(
            err.description(),
            "Syntax error at line 42: unexpected token"
        );
    }

    #[test]
    fn test_syntax_error_without_line() {
        let err = LError::syntax_error("unexpected token", None);
        assert_eq!(err.description(), "Syntax error: unexpected token");
    }

    #[test]
    fn test_compile_error() {
        let err = LError::compile_error("invalid expression");
        assert_eq!(err.description(), "Compile error: invalid expression");
    }

    #[test]
    fn test_macro_error() {
        let err = LError::macro_error("macro expansion failed");
        assert_eq!(err.description(), "Macro error: macro expansion failed");
    }

    #[test]
    fn test_file_not_found_error() {
        let err = LError::file_not_found("script.lisp");
        assert_eq!(err.description(), "File not found: script.lisp");
    }

    #[test]
    fn test_error_display_trait() {
        let err = LError::undefined_variable("x");
        let display = format!("{}", err);
        assert_eq!(display, "Reference error: undefined variable 'x'");
    }

    #[test]
    fn test_error_to_string_conversion() {
        let err = LError::type_mismatch("int", "bool");
        let s: String = err.into();
        assert_eq!(s, "Type error: expected int, got bool");
    }

    #[test]
    fn test_string_to_error_conversion() {
        let err: LError = "some error message".to_string().into();
        assert_eq!(err.description(), "Error: some error message");
    }

    #[test]
    fn test_str_to_error_conversion() {
        let err: LError = "some error".into();
        assert_eq!(err.description(), "Error: some error");
    }

    #[test]
    fn test_error_display_with_location() {
        let loc = SourceLoc::from_line_col(42, 13);
        let err = LError::undefined_variable("x").with_location(loc);
        let display = format!("{}", err);
        assert!(display.contains("Reference error"));
        assert!(display.contains("42:13"));
    }

    #[test]
    fn test_error_debug_format() {
        let err = LError::division_by_zero();
        let debug = format!("{:?}", err);
        assert!(debug.contains("DivisionByZero"));
    }

    #[test]
    fn test_ffi_type_error() {
        let err = LError::ffi_type_error("struct Point", "invalid field offset");
        assert_eq!(
            err.description(),
            "FFI type error for struct Point: invalid field offset"
        );
    }

    #[test]
    fn test_invalid_numeric_operation() {
        let err = LError::invalid_numeric_operation("sqrt", "negative number");
        assert_eq!(
            err.description(),
            "Arithmetic error in sqrt: negative number"
        );
    }

    #[test]
    fn test_pattern_error() {
        let err = LError::pattern_error("unreachable pattern");
        assert_eq!(err.description(), "Pattern error: unreachable pattern");
    }

    #[test]
    fn test_uncaught_exception() {
        let err = LError::uncaught_exception("user exception");
        assert_eq!(err.description(), "Uncaught exception: user exception");
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
        let err: Box<dyn StdError> = Box::new(LError::division_by_zero());
        assert_eq!(err.to_string(), "Arithmetic error: division by zero");
    }

    #[test]
    fn test_multiple_error_types_different() {
        let err1 = LError::type_mismatch("int", "string");
        let err2 = LError::undefined_variable("x");
        assert_ne!(format!("{:?}", err1), format!("{:?}", err2));
    }

    #[test]
    fn test_argument_error() {
        let err = LError::argument_error("invalid format string");
        assert_eq!(err.description(), "Argument error: invalid format string");
    }

    #[test]
    fn test_execution_error() {
        let err = LError::execution_error("infinite loop detected");
        assert_eq!(err.description(), "Execution error: infinite loop detected");
    }

    #[test]
    fn test_file_read_error() {
        let err = LError::file_read_error("file.lisp", "permission denied");
        assert_eq!(
            err.description(),
            "Failed to read file file.lisp: permission denied"
        );
    }

    #[test]
    fn test_generic_error() {
        let err = LError::generic("something went wrong");
        assert_eq!(err.description(), "Error: something went wrong");
    }
}
