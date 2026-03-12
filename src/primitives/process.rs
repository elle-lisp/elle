//! Process-related primitives
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_HALT, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Exit the process with an optional exit code
///
/// (exit)       ; exits with code 0
/// (exit 0)     ; exits with code 0
/// (exit 1)     ; exits with code 1
/// (exit 42)    ; exits with code 42
pub(crate) fn prim_exit(args: &[Value]) -> (SignalBits, Value) {
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
pub(crate) fn prim_halt(args: &[Value]) -> (SignalBits, Value) {
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

/// Return command-line arguments as an array, excluding the interpreter
/// and script path (argv\[0\] and argv\[1\]).
///
/// (sys/args) => ["arg1" "arg2" ...]
pub(crate) fn prim_sys_args(_args: &[Value]) -> (SignalBits, Value) {
    let args: Vec<Value> = std::env::args().skip(2).map(Value::string).collect();
    (SIG_OK, Value::array(args))
}

/// Declarative primitive definitions for process operations
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "sys/exit",
        func: prim_exit,
        signal: Signal::errors(),
        arity: Arity::Range(0, 1),
        doc: "Exit the process with an optional exit code (0-255)",
        params: &["code"],
        category: "sys",
        example: "(sys/exit 0)",
        aliases: &["exit", "os/exit"],
    },
    PrimitiveDef {
        name: "sys/halt",
        func: prim_halt,
        signal: Signal::halts(),
        arity: Arity::Range(0, 1),
        doc: "Halt the VM gracefully, returning a value to the host",
        params: &["value"],
        category: "sys",
        example: "(sys/halt 42)",
        aliases: &["halt", "os/halt"],
    },
    PrimitiveDef {
        name: "sys/args",
        func: prim_sys_args,
        signal: Signal::inert(),
        arity: Arity::Exact(0),
        doc: "Return command-line arguments as an array (excluding interpreter and script path)",
        params: &[],
        category: "sys",
        example: "(sys/args)",
        aliases: &[],
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
