//! Process-related primitives
use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_HALT};
use crate::value::types::Arity;
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

/// Halt the VM gracefully, returning a value to the host.
///
/// (halt)         ; halts with nil
/// (halt value)   ; halts with value
///
/// Unlike `exit`, `halt` does not terminate the process. It signals the
/// VM to stop execution and return the value to the caller. The signal
/// is maskable by fiber signal masks but non-resumable: once a fiber
/// halts, it is Dead.
pub fn prim_halt(args: &[Value]) -> (SignalBits, Value) {
    if args.len() > 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("halt: expected 0-1 arguments, got {}", args.len()),
            ),
        );
    }
    let value = if args.is_empty() { Value::NIL } else { args[0] };
    (SIG_HALT, value)
}

/// Declarative primitive definitions for process operations
pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "os/exit",
        func: prim_exit,
        effect: Effect::raises(),
        arity: Arity::Range(0, 1),
        doc: "Exit the process with an optional exit code (0-255)",
        params: &["code"],
        category: "os",
        example: "(os/exit 0)",
        aliases: &["exit"],
    },
    PrimitiveDef {
        name: "os/halt",
        func: prim_halt,
        effect: Effect::halts(),
        arity: Arity::Range(0, 1),
        doc: "Halt the VM gracefully, returning a value to the host",
        params: &["value"],
        category: "os",
        example: "(os/halt 42)",
        aliases: &["halt"],
    },
];

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

    #[test]
    fn test_halt_no_args() {
        let (signal, value) = prim_halt(&[]);
        assert_eq!(signal, SIG_HALT);
        assert!(value.is_nil());
    }

    #[test]
    fn test_halt_with_value() {
        let (signal, value) = prim_halt(&[Value::int(42)]);
        assert_eq!(signal, SIG_HALT);
        assert_eq!(value, Value::int(42));
    }

    #[test]
    fn test_halt_too_many_args() {
        let (signal, _) = prim_halt(&[Value::int(0), Value::int(1)]);
        assert_eq!(signal, SIG_ERROR);
    }
}
