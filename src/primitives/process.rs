//! Process-related primitives
use crate::value::fiber::{SignalBits, SIG_ERROR};
use crate::value::{error_val, Value};

/// Exit the process with an optional exit code
///
/// (exit)       ; exits with code 0
/// (exit 0)     ; exits with code 0
/// (exit 1)     ; exits with code 1
/// (exit 42)    ; exits with code 42
pub fn prim_exit(args: &[Value]) -> (SignalBits, Value) {
    let code = if args.is_empty() {
        0
    } else if args.len() == 1 {
        if let Some(n) = args[0].as_int() {
            if !(0..=255).contains(&n) {
                return (
                    SIG_ERROR,
                    error_val(
                        "error",
                        format!("exit: code must be between 0 and 255, got {}", n),
                    ),
                );
            }
            n as i32
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("exit: expected integer, got {}", args[0].type_name()),
                ),
            );
        }
    } else {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("exit: expected 0-1 arguments, got {}", args.len()),
            ),
        );
    };

    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exit_too_many_args() {
        let (signal, _) = prim_exit(&[Value::int(0), Value::int(1)]);
        assert_eq!(signal, SIG_ERROR);
    }

    #[test]
    fn test_exit_wrong_type() {
        let (signal, _) = prim_exit(&[Value::bool(true)]);
        assert_eq!(signal, SIG_ERROR);
    }

    #[test]
    fn test_exit_negative() {
        let (signal, _) = prim_exit(&[Value::int(-1)]);
        assert_eq!(signal, SIG_ERROR);
    }

    #[test]
    fn test_exit_too_large() {
        let (signal, _) = prim_exit(&[Value::int(256)]);
        assert_eq!(signal, SIG_ERROR);
    }
}
