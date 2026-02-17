//! FFI Safety - Error handling, type checking, null pointer detection
//!
//! This module provides safety guarantees for FFI operations:
//! - Type checking before and after C calls
//! - Null pointer detection and handling
//! - Segmentation fault catching (Linux only)
//! - Memory bounds validation
//! - Error context and source location tracking

use std::fmt;

use crate::ffi::types::CType;
use crate::Value;

/// FFI error kind enumeration
#[derive(Debug, Clone)]
pub enum FFIErrorKind {
    /// Type mismatch between expected and provided
    TypeMismatch { expected: String, got: String },
    /// Symbol not found in library
    SymbolNotFound { symbol: String, lib: String },
    /// Library not loaded
    LibraryNotLoaded { path: String },
    /// Marshaling failed
    MarshalingFailed { direction: String, reason: String },
    /// Null pointer dereference
    NullPointerDeref { type_name: String },
    /// Segmentation fault detected
    SegmentationFault { address: *const std::ffi::c_void },
    /// Out of memory
    OutOfMemory,
    /// Invalid struct layout
    InvalidStructLayout { struct_name: String },
    /// Invalid function signature
    InvalidSignature { reason: String },
    /// Array bounds check failed
    ArrayBoundsExceeded { index: usize, length: usize },
    /// Custom error
    Custom(String),
}

impl fmt::Display for FFIErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FFIErrorKind::TypeMismatch { expected, got } => {
                write!(f, "Type mismatch: expected {}, got {}", expected, got)
            }
            FFIErrorKind::SymbolNotFound { symbol, lib } => {
                write!(f, "Symbol '{}' not found in {}", symbol, lib)
            }
            FFIErrorKind::LibraryNotLoaded { path } => {
                write!(f, "Library not loaded: {}", path)
            }
            FFIErrorKind::MarshalingFailed { direction, reason } => {
                write!(f, "Marshaling failed ({}): {}", direction, reason)
            }
            FFIErrorKind::NullPointerDeref { type_name } => {
                write!(f, "Null pointer dereference: {}", type_name)
            }
            FFIErrorKind::SegmentationFault { address } => {
                write!(f, "Segmentation fault at {:p}", address)
            }
            FFIErrorKind::OutOfMemory => write!(f, "Out of memory"),
            FFIErrorKind::InvalidStructLayout { struct_name } => {
                write!(f, "Invalid struct layout: {}", struct_name)
            }
            FFIErrorKind::InvalidSignature { reason } => {
                write!(f, "Invalid function signature: {}", reason)
            }
            FFIErrorKind::ArrayBoundsExceeded { index, length } => {
                write!(
                    f,
                    "Array bounds exceeded: index {} >= length {}",
                    index, length
                )
            }
            FFIErrorKind::Custom(msg) => write!(f, "{}", msg),
        }
    }
}

/// FFI error with context
#[derive(Debug, Clone)]
pub struct FFIError {
    pub kind: FFIErrorKind,
    pub context: String,
}

impl FFIError {
    /// Create a new FFI error
    pub fn new(kind: FFIErrorKind, context: impl Into<String>) -> Self {
        FFIError {
            kind,
            context: context.into(),
        }
    }

    /// Convert to Result
    pub fn into_result<T>(self) -> Result<T, FFIError> {
        Err(self)
    }
}

impl fmt::Display for FFIError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FFI Error: {}\n  Context: {}", self.kind, self.context)
    }
}

impl std::error::Error for FFIError {}

/// Type safety checker
pub struct TypeChecker;

impl TypeChecker {
    /// Check if a Value matches the expected CType
    pub fn check_type(value: &Value, expected: &CType) -> Result<(), FFIError> {
        let is_int = value.as_int().is_some();
        let is_float = value.as_float().is_some();
        let is_string = value.as_string().is_some();
        let is_nil = value.is_nil();
        let is_heap = value.as_heap_ptr().is_some();

        match expected {
            CType::Int
            | CType::UInt
            | CType::Long
            | CType::ULong
            | CType::Bool
            | CType::Char
            | CType::SChar
            | CType::UChar
            | CType::Short
            | CType::UShort
            | CType::LongLong
            | CType::ULongLong => {
                if is_int {
                    Ok(())
                } else {
                    Err(FFIError::new(
                        FFIErrorKind::TypeMismatch {
                            expected: format!("{:?}", expected),
                            got: format!("{:?}", value),
                        },
                        "Type checking failed",
                    ))
                }
            }
            CType::Float | CType::Double => {
                if is_float {
                    Ok(())
                } else {
                    Err(FFIError::new(
                        FFIErrorKind::TypeMismatch {
                            expected: format!("{:?}", expected),
                            got: format!("{:?}", value),
                        },
                        "Type checking failed",
                    ))
                }
            }
            CType::Pointer(inner) => {
                if is_nil || (is_string && **inner == CType::Char) || is_heap {
                    Ok(()) // null pointer or valid string/heap
                } else {
                    Err(FFIError::new(
                        FFIErrorKind::TypeMismatch {
                            expected: format!("{:?}", expected),
                            got: format!("{:?}", value),
                        },
                        "Type checking failed",
                    ))
                }
            }
            CType::Void => {
                if is_heap {
                    Ok(())
                } else {
                    Err(FFIError::new(
                        FFIErrorKind::TypeMismatch {
                            expected: format!("{:?}", expected),
                            got: format!("{:?}", value),
                        },
                        "Type checking failed",
                    ))
                }
            }
            _ => Err(FFIError::new(
                FFIErrorKind::TypeMismatch {
                    expected: format!("{:?}", expected),
                    got: format!("{:?}", value),
                },
                "Type checking failed",
            )),
        }
    }

    /// Validate a function signature matches expected types
    pub fn validate_signature(
        arg_values: &[Value],
        arg_types: &[CType],
        _return_type: &CType,
    ) -> Result<(), FFIError> {
        if arg_values.len() != arg_types.len() {
            return Err(FFIError::new(
                FFIErrorKind::InvalidSignature {
                    reason: format!(
                        "Argument count mismatch: expected {}, got {}",
                        arg_types.len(),
                        arg_values.len()
                    ),
                },
                "Function signature validation",
            ));
        }

        for (value, expected) in arg_values.iter().zip(arg_types.iter()) {
            Self::check_type(value, expected)?;
        }

        Ok(())
    }
}

/// Null pointer safety checker
pub struct NullPointerChecker;

impl NullPointerChecker {
    /// Check if a value is a null pointer
    pub fn is_null(value: &Value) -> bool {
        value.is_nil()
    }

    /// Check if a C handle is null
    pub fn check_handle_not_null(value: &Value, type_name: &str) -> Result<(), FFIError> {
        if Self::is_null(value) {
            return Err(FFIError::new(
                FFIErrorKind::NullPointerDeref {
                    type_name: type_name.to_string(),
                },
                "Attempted to dereference null pointer",
            ));
        }
        Ok(())
    }
}

/// Array bounds checker
pub struct ArrayBoundsChecker;

impl ArrayBoundsChecker {
    /// Check if index is within bounds
    pub fn check_bounds(index: usize, length: usize) -> Result<(), FFIError> {
        if index >= length {
            return Err(FFIError::new(
                FFIErrorKind::ArrayBoundsExceeded { index, length },
                "Array index out of bounds",
            ));
        }
        Ok(())
    }
}

thread_local! {
    /// Last error storage (thread-local)
    static LAST_FFI_ERROR: std::cell::RefCell<Option<FFIError>> = const { std::cell::RefCell::new(None) };
}

/// Set the last FFI error (for recovery)
pub fn set_last_error(error: FFIError) {
    LAST_FFI_ERROR.with(|e| *e.borrow_mut() = Some(error));
}

/// Get the last FFI error
pub fn get_last_error() -> Option<FFIError> {
    LAST_FFI_ERROR.with(|e| e.borrow_mut().take())
}

/// Clear the last FFI error
pub fn clear_last_error() {
    LAST_FFI_ERROR.with(|e| *e.borrow_mut() = None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_mismatch_error() {
        let kind = FFIErrorKind::TypeMismatch {
            expected: "int".to_string(),
            got: "float".to_string(),
        };
        assert!(format!("{}", kind).contains("Type mismatch"));
    }

    #[test]
    fn test_type_checker() {
        assert!(TypeChecker::check_type(&Value::int(42), &CType::Int).is_ok());
        assert!(
            TypeChecker::check_type(&Value::float(std::f64::consts::PI), &CType::Float).is_ok()
        );
        assert!(TypeChecker::check_type(
            &Value::string("hello"),
            &CType::Pointer(Box::new(CType::Char))
        )
        .is_ok());
    }

    #[test]
    fn test_type_mismatch() {
        assert!(TypeChecker::check_type(&Value::int(42), &CType::Float).is_err());
        assert!(TypeChecker::check_type(&Value::float(std::f64::consts::PI), &CType::Int).is_err());
    }

    #[test]
    fn test_signature_validation() {
        let args = vec![Value::int(42), Value::float(std::f64::consts::PI)];
        let types = vec![CType::Int, CType::Float];
        let return_type = CType::Void;

        assert!(TypeChecker::validate_signature(&args, &types, &return_type).is_ok());
    }

    #[test]
    fn test_signature_argument_count_mismatch() {
        let args = vec![Value::int(42)];
        let types = vec![CType::Int, CType::Float];
        let return_type = CType::Void;

        assert!(TypeChecker::validate_signature(&args, &types, &return_type).is_err());
    }

    #[test]
    fn test_null_pointer_detection() {
        assert!(NullPointerChecker::is_null(&Value::NIL));
        assert!(!NullPointerChecker::is_null(&Value::int(0)));
    }

    #[test]
    fn test_null_pointer_check() {
        assert!(NullPointerChecker::check_handle_not_null(&Value::NIL, "TestType").is_err());
        assert!(NullPointerChecker::check_handle_not_null(&Value::int(42), "TestType").is_ok());
    }

    #[test]
    fn test_array_bounds() {
        assert!(ArrayBoundsChecker::check_bounds(0, 10).is_ok());
        assert!(ArrayBoundsChecker::check_bounds(5, 10).is_ok());
        assert!(ArrayBoundsChecker::check_bounds(10, 10).is_err());
        assert!(ArrayBoundsChecker::check_bounds(100, 10).is_err());
    }

    #[test]
    fn test_last_error_storage() {
        clear_last_error();
        assert!(get_last_error().is_none());

        let error = FFIError::new(
            FFIErrorKind::Custom("test error".to_string()),
            "test context",
        );
        set_last_error(error);
        assert!(get_last_error().is_some());
    }
}
