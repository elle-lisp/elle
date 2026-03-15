//! Process-related primitives
use crate::io::request::{IoOp, IoRequest, ProcessHandle, StdioDisposition};
use crate::primitives::def::PrimitiveDef;
use crate::signals::{Signal, SIG_EXEC};
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_HALT, SIG_IO, SIG_OK, SIG_YIELD};
use crate::value::heap::TableKey;
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

/// Parse the optional opts struct for sys/exec.
/// Returns (env, cwd, stdin, stdout, stderr) or an error tuple.
fn parse_exec_opts(
    opts: &Value,
) -> Result<
    (
        Option<Vec<(String, String)>>,
        Option<String>,
        StdioDisposition,
        StdioDisposition,
        StdioDisposition,
    ),
    (SignalBits, Value),
> {
    let fields = match opts.as_struct() {
        Some(f) => f,
        None => {
            return Err((
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("sys/exec: opts must be struct, got {}", opts.type_name()),
                ),
            ))
        }
    };

    // :env — struct of string → string, or nil for inherit
    let env = match fields.get(&TableKey::Keyword("env".into())) {
        Some(v) if v.is_nil() => None,
        Some(v) => {
            let env_fields = match v.as_struct() {
                Some(f) => f,
                None => {
                    return Err((
                        SIG_ERROR,
                        error_val("type-error", "sys/exec: :env must be a struct"),
                    ))
                }
            };
            let mut pairs = Vec::new();
            for (k, val) in env_fields {
                let key_str = match k {
                    TableKey::Keyword(s) => s.clone(),
                    TableKey::String(s) => s.clone(),
                    _ => {
                        return Err((
                            SIG_ERROR,
                            error_val(
                                "type-error",
                                "sys/exec: :env keys must be keywords or strings",
                            ),
                        ))
                    }
                };
                let val_str = match val.with_string(|s| s.to_string()) {
                    Some(s) => s,
                    None => {
                        return Err((
                            SIG_ERROR,
                            error_val("type-error", "sys/exec: :env values must be strings"),
                        ))
                    }
                };
                pairs.push((key_str, val_str));
            }
            Some(pairs)
        }
        None => None,
    };

    // :cwd — string or nil
    let cwd = match fields.get(&TableKey::Keyword("cwd".into())) {
        Some(v) if v.is_nil() => None,
        Some(v) => Some(match v.with_string(|s| s.to_string()) {
            Some(s) => s,
            None => {
                return Err((
                    SIG_ERROR,
                    error_val("type-error", "sys/exec: :cwd must be a string"),
                ))
            }
        }),
        None => None,
    };

    // :stdin / :stdout / :stderr — keywords :pipe, :inherit, :null
    fn parse_disp(v: &Value, field: &str) -> Result<StdioDisposition, (SignalBits, Value)> {
        match v.as_keyword_name() {
            Some("pipe") => Ok(StdioDisposition::Pipe),
            Some("inherit") => Ok(StdioDisposition::Inherit),
            Some("null") => Ok(StdioDisposition::Null),
            _ => Err((
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("sys/exec: {} must be :pipe, :inherit, or :null", field),
                ),
            )),
        }
    }

    let stdin_disp = match fields.get(&TableKey::Keyword("stdin".into())) {
        Some(v) => parse_disp(v, ":stdin")?,
        None => StdioDisposition::Pipe,
    };
    let stdout_disp = match fields.get(&TableKey::Keyword("stdout".into())) {
        Some(v) => parse_disp(v, ":stdout")?,
        None => StdioDisposition::Pipe,
    };
    let stderr_disp = match fields.get(&TableKey::Keyword("stderr".into())) {
        Some(v) => parse_disp(v, ":stderr")?,
        None => StdioDisposition::Pipe,
    };

    Ok((env, cwd, stdin_disp, stdout_disp, stderr_disp))
}

/// Extract a ProcessHandle Value from either:
/// - A Value with external_type_name "process" (direct handle)
/// - A struct with a :process key containing the handle
fn extract_process_handle(val: &Value, fn_name: &str) -> Result<Value, (SignalBits, Value)> {
    if val.external_type_name() == Some("process") {
        return Ok(*val);
    }
    if let Some(fields) = val.as_struct() {
        match fields.get(&TableKey::Keyword("process".into())) {
            Some(v) => return Ok(*v),
            None => {
                return Err((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("{}: struct has no :process key", fn_name),
                    ),
                ))
            }
        }
    }
    Err((
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "{}: expected process handle or exec result struct, got {}",
                fn_name,
                val.type_name()
            ),
        ),
    ))
}

/// Spawn a subprocess, returning an IoRequest that the scheduler will execute.
///
/// (sys/exec program args)
/// (sys/exec program args opts)
///
/// Returns (SIG_EXEC | SIG_IO | SIG_YIELD, io-request).
fn prim_sys_exec(args: &[Value]) -> (SignalBits, Value) {
    if args.len() < 2 || args.len() > 3 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("sys/exec: expected 2-3 arguments, got {}", args.len()),
            ),
        );
    }

    let program = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "sys/exec: program must be string, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };

    let arg_vals = match args[1].as_array() {
        Some(a) => a,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("sys/exec: args must be array, got {}", args[1].type_name()),
                ),
            )
        }
    };
    let mut exec_args = Vec::new();
    for v in arg_vals.iter() {
        match v.with_string(|s| s.to_string()) {
            Some(s) => exec_args.push(s),
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("sys/exec: args must be strings, got {}", v.type_name()),
                    ),
                )
            }
        }
    }

    let (env, cwd, stdin_disp, stdout_disp, stderr_disp) = if args.len() > 2 {
        match parse_exec_opts(&args[2]) {
            Ok(opts) => opts,
            Err(e) => return e,
        }
    } else {
        (
            None,
            None,
            StdioDisposition::Pipe,
            StdioDisposition::Pipe,
            StdioDisposition::Pipe,
        )
    };

    let request = IoRequest::portless(IoOp::Spawn {
        program,
        args: exec_args,
        env,
        cwd,
        stdin: stdin_disp,
        stdout: stdout_disp,
        stderr: stderr_disp,
    });
    (SIG_YIELD | SIG_IO | SIG_EXEC, request)
}

/// Wait for a subprocess to exit, returning an IoRequest that the scheduler executes.
///
/// (sys/wait handle-or-struct) → exit-code
///
/// Returns (SIG_EXEC | SIG_IO | SIG_YIELD, io-request).
fn prim_sys_wait(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("sys/wait: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let handle_val = match extract_process_handle(&args[0], "sys/wait") {
        Ok(v) => v,
        Err(e) => return e,
    };
    // Validate it's actually a ProcessHandle (not just any external)
    if handle_val.as_external::<ProcessHandle>().is_none() {
        return (
            SIG_ERROR,
            error_val("type-error", "sys/wait: invalid process handle"),
        );
    }
    let request = IoRequest::new(IoOp::ProcessWait, handle_val);
    (SIG_YIELD | SIG_IO | SIG_EXEC, request)
}

/// Map a signal name keyword (without the colon) to its libc constant.
fn keyword_to_signal(name: &str) -> Option<libc::c_int> {
    match name {
        "sigterm" => Some(libc::SIGTERM),
        "sigkill" => Some(libc::SIGKILL),
        "sighup" => Some(libc::SIGHUP),
        "sigint" => Some(libc::SIGINT),
        "sigquit" => Some(libc::SIGQUIT),
        "sigpipe" => Some(libc::SIGPIPE),
        "sigalrm" => Some(libc::SIGALRM),
        "sigusr1" => Some(libc::SIGUSR1),
        "sigusr2" => Some(libc::SIGUSR2),
        "sigchld" => Some(libc::SIGCHLD),
        "sigcont" => Some(libc::SIGCONT),
        "sigstop" => Some(libc::SIGSTOP),
        "sigtstp" => Some(libc::SIGTSTP),
        "sigttin" => Some(libc::SIGTTIN),
        "sigttou" => Some(libc::SIGTTOU),
        "sigwinch" => Some(libc::SIGWINCH),
        _ => None,
    }
}

/// Send a signal to a subprocess.
///
/// (sys/kill handle-or-struct)           ; sends SIGTERM
/// (sys/kill handle-or-struct 15)        ; integer signal number
/// (sys/kill handle-or-struct :sigterm)  ; keyword signal name
///
/// Synchronous — returns (SIG_OK, nil) on success, (SIG_ERROR, error) on failure.
fn prim_sys_kill(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() || args.len() > 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("sys/kill: expected 1-2 arguments, got {}", args.len()),
            ),
        );
    }
    let handle_val = match extract_process_handle(&args[0], "sys/kill") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let handle = match handle_val.as_external::<ProcessHandle>() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                error_val("type-error", "sys/kill: invalid process handle"),
            )
        }
    };
    let signal = if args.len() > 1 {
        if let Some(n) = args[1].as_int() {
            n as i32
        } else if let Some(name) = args[1].as_keyword_name() {
            match keyword_to_signal(name) {
                Some(sig) => sig,
                None => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!(
                                "sys/kill: unknown signal keyword :{name}; expected integer or one of :sigterm, :sigkill, :sighup, :sigint, :sigquit, :sigpipe, :sigalrm, :sigusr1, :sigusr2, :sigchld, :sigcont, :sigstop, :sigtstp, :sigttin, :sigttou, :sigwinch"
                            ),
                        ),
                    )
                }
            }
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "sys/kill: signal must be integer or keyword, got {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    } else {
        libc::SIGTERM
    };
    let ret = unsafe { libc::kill(handle.pid() as i32, signal) };
    if ret < 0 {
        (
            SIG_ERROR,
            error_val(
                "exec-error",
                format!("sys/kill: {}", std::io::Error::last_os_error()),
            ),
        )
    } else {
        (SIG_OK, Value::NIL)
    }
}

/// Return the OS process ID of a subprocess.
///
/// (process/pid handle-or-struct) → int
///
/// Synchronous — no yield.
fn prim_process_pid(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("process/pid: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let handle_val = match extract_process_handle(&args[0], "process/pid") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let handle = match handle_val.as_external::<ProcessHandle>() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                error_val("type-error", "process/pid: invalid process handle"),
            )
        }
    };
    (SIG_OK, Value::int(handle.pid() as i64))
}

/// Declarative primitive definitions for process operations
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "sys/exit",
        func: prim_exit,
        signal: Signal::halts(),
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
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "Return command-line arguments as an array (excluding interpreter and script path)",
        params: &[],
        category: "sys",
        example: "(sys/args)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "sys/exec",
        func: prim_sys_exec,
        signal: Signal {
            // SIG_EXEC: capability bit for fiber mask access control.
            // SIG_IO: dispatch bit — routes through the I/O scheduler.
            // Both are emitted; dispatch is IO-based; exec bit enables capability gating.
            bits: SignalBits::new(SIG_ERROR.0 | SIG_YIELD.0 | SIG_IO.0 | SIG_EXEC.0),
            propagates: 0,
        },
        arity: Arity::Range(2, 3),
        doc: "Spawn a subprocess. Returns {:pid int :stdin port|nil :stdout port|nil :stderr port|nil :process <process>}",
        params: &["program", "args", "opts"],
        category: "sys",
        example: "(sys/exec \"ls\" [\"-la\"])",
        aliases: &[],
    },
    PrimitiveDef {
        name: "sys/wait",
        func: prim_sys_wait,
        signal: Signal {
            // SIG_EXEC: capability bit (same fiber mask semantics as sys/exec).
            bits: SignalBits::new(SIG_ERROR.0 | SIG_YIELD.0 | SIG_IO.0 | SIG_EXEC.0),
            propagates: 0,
        },
        arity: Arity::Exact(1),
        doc: "Wait for a subprocess to exit. Returns exit code (0 = success).",
        params: &["handle"],
        category: "sys",
        example: "(sys/wait proc)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "sys/kill",
        func: prim_sys_kill,
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Send a signal to a subprocess. signal is an integer or a keyword like :sigterm, :sigkill, :sighup, :sigint, :sigquit, :sigpipe, :sigalrm, :sigusr1, :sigusr2, :sigchld, :sigcont, :sigstop, :sigtstp, :sigttin, :sigttou, :sigwinch (default: :sigterm).",
        params: &["handle", "signal"],
        category: "sys",
        example: "(sys/kill proc :sigterm)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "process/pid",
        func: prim_process_pid,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the OS process ID of a subprocess.",
        params: &["handle"],
        category: "sys",
        example: "(process/pid proc)",
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

    // --- sys/exec ---

    #[test]
    fn test_sys_exec_arity_too_few() {
        let (sig, _) = prim_sys_exec(&[]);
        assert_eq!(sig, SIG_ERROR);
        let (sig, _) = prim_sys_exec(&[Value::string("echo")]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_sys_exec_arity_too_many() {
        let (sig, _) = prim_sys_exec(&[
            Value::string("echo"),
            Value::array(vec![]),
            Value::struct_from(std::collections::BTreeMap::new()),
            Value::string("extra"),
        ]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_sys_exec_program_not_string() {
        let (sig, _) = prim_sys_exec(&[Value::int(42), Value::array(vec![])]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_sys_exec_args_not_array() {
        let (sig, _) = prim_sys_exec(&[Value::string("echo"), Value::string("not-array")]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_sys_exec_args_element_not_string() {
        let (sig, _) = prim_sys_exec(&[Value::string("echo"), Value::array(vec![Value::int(99)])]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_sys_exec_returns_sig_exec_io_yield() {
        let (sig, val) = prim_sys_exec(&[
            Value::string("echo"),
            Value::array(vec![Value::string("hi")]),
        ]);
        assert!(sig.contains(SIG_EXEC), "expected SIG_EXEC in {:?}", sig);
        assert!(sig.contains(SIG_IO), "expected SIG_IO in {:?}", sig);
        assert!(sig.contains(SIG_YIELD), "expected SIG_YIELD in {:?}", sig);
        assert_eq!(val.external_type_name(), Some("io-request"));
    }

    // --- sys/wait ---

    #[test]
    fn test_sys_wait_arity() {
        let (sig, _) = prim_sys_wait(&[]);
        assert_eq!(sig, SIG_ERROR);
        let (sig, _) = prim_sys_wait(&[Value::NIL, Value::NIL]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_sys_wait_wrong_type() {
        let (sig, _) = prim_sys_wait(&[Value::int(42)]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_sys_wait_signal_bits() {
        use crate::io::request::ProcessHandle;
        use std::process::Command;
        let child = Command::new("/bin/true").spawn().unwrap();
        let pid = child.id();
        let handle = ProcessHandle::new(pid, child);
        let handle_val = Value::external("process", handle);
        let (sig, val) = prim_sys_wait(&[handle_val]);
        assert!(sig.contains(SIG_EXEC), "expected SIG_EXEC in {:?}", sig);
        assert!(sig.contains(SIG_IO), "expected SIG_IO in {:?}", sig);
        assert!(sig.contains(SIG_YIELD));
        assert_eq!(val.external_type_name(), Some("io-request"));
    }

    // --- sys/kill ---

    #[test]
    fn test_sys_kill_arity() {
        let (sig, _) = prim_sys_kill(&[]);
        assert_eq!(sig, SIG_ERROR);
        let (sig, _) = prim_sys_kill(&[Value::NIL, Value::NIL, Value::NIL]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_sys_kill_wrong_type() {
        // Single int arg — not a process handle or struct, triggers type error
        let (sig, _) = prim_sys_kill(&[Value::int(42)]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_sys_kill_signal_is_errors() {
        // sys/kill is synchronous — it does not yield.
        let def = PRIMITIVES.iter().find(|d| d.name == "sys/kill").unwrap();
        assert!(!def.signal.may_yield(), "sys/kill must not yield");
        assert!(def.signal.may_error());
    }

    #[test]
    fn test_sys_kill_keyword_sigterm() {
        use crate::io::request::ProcessHandle;
        use std::process::Command;
        let child = Command::new("/bin/sleep").arg("100").spawn().unwrap();
        let pid = child.id();
        let handle = ProcessHandle::new(pid, child);
        let handle_val = Value::external("process", handle);
        let sig_kw = Value::keyword("sigterm");
        let (sig, _) = prim_sys_kill(&[handle_val, sig_kw]);
        assert_eq!(sig, SIG_OK);
    }

    #[test]
    fn test_sys_kill_unknown_keyword() {
        use crate::io::request::ProcessHandle;
        use std::process::Command;
        let child = Command::new("/bin/sleep").arg("100").spawn().unwrap();
        let pid = child.id();
        let handle = ProcessHandle::new(pid, child);
        let handle_val = Value::external("process", handle);
        let sig_kw = Value::keyword("sigfoo");
        let (sig, _) = prim_sys_kill(&[handle_val, sig_kw]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_sys_kill_integer_still_works() {
        use crate::io::request::ProcessHandle;
        use std::process::Command;
        let child = Command::new("/bin/sleep").arg("100").spawn().unwrap();
        let pid = child.id();
        let handle = ProcessHandle::new(pid, child);
        let handle_val = Value::external("process", handle);
        let (sig, _) = prim_sys_kill(&[handle_val, Value::int(15)]);
        assert_eq!(sig, SIG_OK);
    }

    // --- process/pid ---

    #[test]
    fn test_process_pid_arity() {
        let (sig, _) = prim_process_pid(&[]);
        assert_eq!(sig, SIG_ERROR);
        let (sig, _) = prim_process_pid(&[Value::NIL, Value::NIL]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_process_pid_wrong_type() {
        let (sig, _) = prim_process_pid(&[Value::int(42)]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_process_pid_from_handle() {
        use crate::io::request::ProcessHandle;
        use std::process::Command;
        let child = Command::new("/bin/true").spawn().unwrap();
        let expected_pid = child.id();
        let handle = ProcessHandle::new(expected_pid, child);
        let handle_val = Value::external("process", handle);
        let (sig, val) = prim_process_pid(&[handle_val]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(val.as_int(), Some(expected_pid as i64));
    }

    #[test]
    fn test_process_pid_from_struct() {
        use crate::io::request::ProcessHandle;
        use crate::value::heap::TableKey;
        use std::collections::BTreeMap;
        use std::process::Command;
        let child = Command::new("/bin/true").spawn().unwrap();
        let expected_pid = child.id();
        let handle = ProcessHandle::new(expected_pid, child);
        let handle_val = Value::external("process", handle);
        let mut fields = BTreeMap::new();
        fields.insert(TableKey::Keyword("process".into()), handle_val);
        fields.insert(
            TableKey::Keyword("pid".into()),
            Value::int(expected_pid as i64),
        );
        let proc_struct = Value::struct_from(fields);
        let (sig, val) = prim_process_pid(&[proc_struct]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(val.as_int(), Some(expected_pid as i64));
    }
}
