//! Process-related primitives

use crate::value::Value;

/// Exit the process with an optional exit code
///
/// (exit)       ; exits with code 0
/// (exit 0)     ; exits with code 0
/// (exit 1)     ; exits with code 1
/// (exit 42)    ; exits with code 42
pub fn prim_exit(args: &[Value]) -> Result<Value, String> {
    let code = if args.is_empty() {
        0
    } else if args.len() == 1 {
        match &args[0] {
            Value::Int(n) => {
                if *n < 0 || *n > 255 {
                    return Err(format!("exit code must be between 0 and 255, got {}", n));
                }
                *n as i32
            }
            _ => {
                return Err(format!(
                    "exit requires an integer argument, got {}",
                    args[0].type_name()
                ));
            }
        }
    } else {
        return Err(format!(
            "exit requires 0 or 1 arguments, got {}",
            args.len()
        ));
    };

    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exit_too_many_args() {
        let result = prim_exit(&[Value::Int(0), Value::Int(1)]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("0 or 1 arguments"));
    }

    #[test]
    fn test_exit_wrong_type() {
        let result = prim_exit(&[Value::Bool(true)]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("integer argument"));
    }

    #[test]
    fn test_exit_negative() {
        let result = prim_exit(&[Value::Int(-1)]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("between 0 and 255"));
    }

    #[test]
    fn test_exit_too_large() {
        let result = prim_exit(&[Value::Int(256)]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("between 0 and 255"));
    }
}
