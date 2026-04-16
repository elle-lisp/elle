//! Subprocess-related primitives
use crate::io::request::{IoOp, IoRequest, ProcessHandle, SpawnRequest, StdioDisposition};
use crate::primitives::def::PrimitiveDef;
use crate::signals::{Signal, SIG_EXEC};
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_HALT, SIG_IO, SIG_OK, SIG_YIELD};
use crate::value::heap::TableKey;
use crate::value::types::Arity;
use crate::value::{error_val, list, sorted_struct_get, Value};

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
                        "argument-error",
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

/// Return user-provided command-line arguments as a list.
/// Arguments are those that follow the source file (or `-` for stdin)
/// in the process argv. Returns an empty list if no args follow.
///
/// (sys/args) => ("arg1" "arg2" ...)
///
/// Example invocation: elle script.lisp foo bar
///   => sys/args returns ("foo" "bar")
pub(crate) fn prim_sys_args(_args: &[Value]) -> (SignalBits, Value) {
    let user_args: Vec<Value> = match crate::context::get_vm_context() {
        Some(ptr) => {
            let vm = unsafe { &*ptr };
            vm.user_args
                .iter()
                .map(|s| Value::string(s.as_str()))
                .collect()
        }
        None => vec![],
    };
    (SIG_OK, list(user_args))
}

/// Return the full argv as a list: script name followed by all user args.
/// Element 0 is the script name (or "-" for stdin).
/// Returns an empty list in REPL mode (when no source file was given).
///
/// (sys/argv) => ("-" "arg1" "arg2" ...)   ; stdin
/// (sys/argv) => ("script.lisp" "arg1" ...) ; file
/// (sys/argv) => ()                          ; REPL
///
/// Example invocation: elle - foo bar
///   => sys/argv returns ("-" "foo" "bar")
pub(crate) fn prim_sys_argv(_args: &[Value]) -> (SignalBits, Value) {
    match crate::context::get_vm_context() {
        Some(ptr) => {
            let vm = unsafe { &*ptr };
            if vm.source_arg.is_empty() {
                return (SIG_OK, Value::EMPTY_LIST);
            }
            let mut all: Vec<Value> = Vec::with_capacity(1 + vm.user_args.len());
            all.push(Value::string(vm.source_arg.as_str()));
            for s in &vm.user_args {
                all.push(Value::string(s.as_str()));
            }
            (SIG_OK, list(all))
        }
        None => (SIG_OK, Value::EMPTY_LIST),
    }
}

/// Return the process environment as an immutable struct, or look up a single variable.
/// Keys are strings (env var names as-is), values are strings.
/// Non-UTF-8 keys or values are silently skipped.
///
/// (sys/env) => {"HOME" "/home/user" "PATH" "/usr/bin:..." ...}
/// (sys/env "HOME") => "/home/user" or nil if not set
pub(crate) fn prim_sys_env(args: &[Value]) -> (SignalBits, Value) {
    if args.len() > 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("sys/env: expected 0-1 arguments, got {}", args.len()),
            ),
        );
    }
    if args.len() == 1 {
        let name = match args[0].with_string(|s| s.to_string()) {
            Some(s) => s,
            None => {
                return (
                    SIG_ERROR,
                    error_val("type-error", "sys/env: expected string argument"),
                )
            }
        };
        return match std::env::var(&name) {
            Ok(val) => (SIG_OK, Value::string(&*val)),
            Err(_) => (SIG_OK, Value::NIL),
        };
    }
    let mut fields: std::collections::BTreeMap<TableKey, Value> = std::collections::BTreeMap::new();
    for (key, val) in
        std::env::vars_os().filter_map(|(k, v)| k.into_string().ok().zip(v.into_string().ok()))
    {
        fields.insert(TableKey::String(key), Value::string(val));
    }
    (SIG_OK, Value::struct_from(fields))
}

/// Parsed subprocess options: (env, cwd, stdin, stdout, stderr).
type ExecOpts = (
    Option<Vec<(String, String)>>,
    Option<String>,
    StdioDisposition,
    StdioDisposition,
    StdioDisposition,
);

/// Parse the optional opts struct for subprocess/exec.
/// Returns (env, cwd, stdin, stdout, stderr) or an error tuple.
fn parse_exec_opts(opts: &Value) -> Result<ExecOpts, (SignalBits, Value)> {
    let fields = match opts.as_struct() {
        Some(f) => f,
        None => {
            return Err((
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "subprocess/exec: opts must be struct, got {}",
                        opts.type_name()
                    ),
                ),
            ))
        }
    };

    // :env — struct of string → string, or nil for inherit
    let env = match sorted_struct_get(fields, &TableKey::Keyword("env".into())) {
        Some(v) if v.is_nil() => None,
        Some(v) => {
            let env_fields = match v.as_struct() {
                Some(f) => f,
                None => {
                    return Err((
                        SIG_ERROR,
                        error_val("type-error", "subprocess/exec: :env must be a struct"),
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
                                "subprocess/exec: :env keys must be keywords or strings",
                            ),
                        ))
                    }
                };
                let val_str = match val.with_string(|s| s.to_string()) {
                    Some(s) => s,
                    None => {
                        return Err((
                            SIG_ERROR,
                            error_val("type-error", "subprocess/exec: :env values must be strings"),
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
    let cwd = match sorted_struct_get(fields, &TableKey::Keyword("cwd".into())) {
        Some(v) if v.is_nil() => None,
        Some(v) => Some(match v.with_string(|s| s.to_string()) {
            Some(s) => s,
            None => {
                return Err((
                    SIG_ERROR,
                    error_val("type-error", "subprocess/exec: :cwd must be a string"),
                ))
            }
        }),
        None => None,
    };

    // :stdin / :stdout / :stderr — keywords :pipe, :inherit, :null
    fn parse_disp(v: &Value, field: &str) -> Result<StdioDisposition, (SignalBits, Value)> {
        match v.as_keyword_name().as_deref() {
            Some("pipe") => Ok(StdioDisposition::Pipe),
            Some("inherit") => Ok(StdioDisposition::Inherit),
            Some("null") => Ok(StdioDisposition::Null),
            _ => Err((
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "subprocess/exec: {} must be :pipe, :inherit, or :null",
                        field
                    ),
                ),
            )),
        }
    }

    let stdin_disp = match sorted_struct_get(fields, &TableKey::Keyword("stdin".into())) {
        Some(v) => parse_disp(v, ":stdin")?,
        None => StdioDisposition::Pipe,
    };
    let stdout_disp = match sorted_struct_get(fields, &TableKey::Keyword("stdout".into())) {
        Some(v) => parse_disp(v, ":stdout")?,
        None => StdioDisposition::Pipe,
    };
    let stderr_disp = match sorted_struct_get(fields, &TableKey::Keyword("stderr".into())) {
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
        match sorted_struct_get(fields, &TableKey::Keyword("process".into())) {
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

/// Extract a `Vec<String>` from a sequence value (empty list, cons list,
/// array, or mutable array). Each element must be a string.
/// Returns `Err((SIG_ERROR, error_val(...)))` on type mismatch.
fn extract_string_sequence(seq: &Value, fn_name: &str) -> Result<Vec<String>, (SignalBits, Value)> {
    let mut result = Vec::new();

    // Empty list — zero args
    if seq.is_empty_list() {
        return Ok(result);
    }

    // Cons list (proper only)
    if seq.as_cons().is_some() {
        let mut current = *seq;
        loop {
            if current.is_empty_list() {
                break;
            }
            match current.as_cons() {
                Some(cons) => {
                    match cons.first.with_string(|s| s.to_string()) {
                        Some(s) => result.push(s),
                        None => {
                            return Err((
                                SIG_ERROR,
                                error_val(
                                    "type-error",
                                    format!(
                                        "{}: args element must be string, got {}",
                                        fn_name,
                                        cons.first.type_name()
                                    ),
                                ),
                            ))
                        }
                    }
                    current = cons.rest;
                }
                None => {
                    return Err((
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!(
                                "{}: improper list ending in {}",
                                fn_name,
                                current.type_name()
                            ),
                        ),
                    ))
                }
            }
        }
        return Ok(result);
    }

    // Immutable array
    if let Some(elems) = seq.as_array() {
        for v in elems.iter() {
            match v.with_string(|s| s.to_string()) {
                Some(s) => result.push(s),
                None => {
                    return Err((
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!(
                                "{}: args element must be string, got {}",
                                fn_name,
                                v.type_name()
                            ),
                        ),
                    ))
                }
            }
        }
        return Ok(result);
    }

    // Mutable array
    if let Some(arr) = seq.as_array_mut() {
        for v in arr.borrow().iter() {
            match v.with_string(|s| s.to_string()) {
                Some(s) => result.push(s),
                None => {
                    return Err((
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!(
                                "{}: args element must be string, got {}",
                                fn_name,
                                v.type_name()
                            ),
                        ),
                    ))
                }
            }
        }
        return Ok(result);
    }

    Err((
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "{}: args must be list, array, or @array, got {}",
                fn_name,
                seq.type_name()
            ),
        ),
    ))
}

/// Spawn a subprocess, returning an IoRequest that the scheduler will execute.
///
/// (subprocess/exec program args)
/// (subprocess/exec program args opts)
///
/// Returns (SIG_EXEC | SIG_IO | SIG_YIELD, io-request).
fn prim_subprocess_exec(args: &[Value]) -> (SignalBits, Value) {
    if args.len() < 2 || args.len() > 3 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "subprocess/exec: expected 2-3 arguments, got {}",
                    args.len()
                ),
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
                        "subprocess/exec: program must be string, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };

    let exec_args = match extract_string_sequence(&args[1], "subprocess/exec") {
        Ok(v) => v,
        Err(e) => return e,
    };

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

    let request = IoRequest::portless(IoOp::Spawn(SpawnRequest {
        program,
        args: exec_args,
        env,
        cwd,
        stdin: stdin_disp,
        stdout: stdout_disp,
        stderr: stderr_disp,
    }));
    (SIG_YIELD | SIG_IO | SIG_EXEC, request)
}

/// Wait for a subprocess to exit, returning an IoRequest that the scheduler executes.
///
/// (subprocess/wait handle-or-struct) → exit-code
///
/// Returns (SIG_EXEC | SIG_IO | SIG_YIELD, io-request).
fn prim_subprocess_wait(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("subprocess/wait: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let handle_val = match extract_process_handle(&args[0], "subprocess/wait") {
        Ok(v) => v,
        Err(e) => return e,
    };
    // Validate it's actually a ProcessHandle (not just any external)
    if handle_val.as_external::<ProcessHandle>().is_none() {
        return (
            SIG_ERROR,
            error_val("type-error", "subprocess/wait: invalid process handle"),
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
/// (subprocess/kill handle-or-struct)           ; sends SIGTERM
/// (subprocess/kill handle-or-struct 15)        ; integer signal number
/// (subprocess/kill handle-or-struct :sigterm)  ; keyword signal name
///
/// Synchronous — returns (SIG_OK, nil) on success, (SIG_ERROR, error) on failure.
fn prim_subprocess_kill(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() || args.len() > 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "subprocess/kill: expected 1-2 arguments, got {}",
                    args.len()
                ),
            ),
        );
    }
    let handle_val = match extract_process_handle(&args[0], "subprocess/kill") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let handle = match handle_val.as_external::<ProcessHandle>() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                error_val("type-error", "subprocess/kill: invalid process handle"),
            )
        }
    };
    let signal = if args.len() > 1 {
        if let Some(n) = args[1].as_int() {
            n as i32
        } else if let Some(name) = args[1].as_keyword_name() {
            match keyword_to_signal(&name) {
                Some(sig) => sig,
                None => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!(
                                "subprocess/kill: unknown signal keyword :{name}; expected integer or one of :sigterm, :sigkill, :sighup, :sigint, :sigquit, :sigpipe, :sigalrm, :sigusr1, :sigusr2, :sigchld, :sigcont, :sigstop, :sigtstp, :sigttin, :sigttou, :sigwinch"
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
                        "subprocess/kill: signal must be integer or keyword, got {}",
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
                format!("subprocess/kill: {}", std::io::Error::last_os_error()),
            ),
        )
    } else {
        (SIG_OK, Value::NIL)
    }
}

/// Return the OS process ID of a subprocess.
///
/// (subprocess/pid handle-or-struct) → int
///
/// Synchronous — no yield.
fn prim_subprocess_pid(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("subprocess/pid: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let handle_val = match extract_process_handle(&args[0], "subprocess/pid") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let handle = match handle_val.as_external::<ProcessHandle>() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                error_val("type-error", "subprocess/pid: invalid process handle"),
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
        doc: "Return command-line arguments as a list (excluding interpreter and script path)",
        params: &[],
        category: "sys",
        example: "(sys/args)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "sys/argv",
        func: prim_sys_argv,
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "Return the full argv as a list: script name as element 0 followed by all user args. Element 0 is \"-\" for stdin or the script path for a file. Returns an empty list in REPL mode.",
        params: &[],
        category: "sys",
        example: "(sys/argv)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "sys/env",
        func: prim_sys_env,
        signal: Signal::silent(),
        arity: Arity::Range(0, 1),
        doc: "Return the process environment as a struct with string keys and string values, or look up a single variable by name. Non-UTF-8 entries are silently skipped.",
        params: &["name"],
        category: "sys",
        example: "(sys/env) ; or (sys/env \"HOME\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "subprocess/exec",
        func: prim_subprocess_exec,
        signal: Signal {
            // SIG_EXEC: capability bit for fiber mask access control.
            // SIG_IO: dispatch bit — routes through the I/O scheduler.
            // Both are emitted; dispatch is IO-based; exec bit enables capability gating.
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO).union(SIG_EXEC),
            propagates: 0,
        },
        arity: Arity::Range(2, 3),
        doc: "Spawn a subprocess. Returns {:pid int :stdin port|nil :stdout port|nil :stderr port|nil :process <process>}",
        params: &["program", "args", "opts"],
        category: "sys",
        example: "(subprocess/exec \"ls\" [\"-la\"])",
        aliases: &[],
    },
    PrimitiveDef {
        name: "subprocess/wait",
        func: prim_subprocess_wait,
        signal: Signal {
            // SIG_EXEC: capability bit (same fiber mask semantics as subprocess/exec).
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO).union(SIG_EXEC),
            propagates: 0,
        },
        arity: Arity::Exact(1),
        doc: "Wait for a subprocess to exit. Returns exit code (0 = success).",
        params: &["handle"],
        category: "sys",
        example: "(subprocess/wait proc)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "subprocess/kill",
        func: prim_subprocess_kill,
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Send a signal to a subprocess. signal is an integer or a keyword like :sigterm, :sigkill, :sighup, :sigint, :sigquit, :sigpipe, :sigalrm, :sigusr1, :sigusr2, :sigchld, :sigcont, :sigstop, :sigtstp, :sigttin, :sigttou, :sigwinch (default: :sigterm).",
        params: &["handle", "signal"],
        category: "sys",
        example: "(subprocess/kill proc :sigterm)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "subprocess/pid",
        func: prim_subprocess_pid,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the OS process ID of a subprocess.",
        params: &["handle"],
        category: "sys",
        example: "(subprocess/pid proc)",
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

    // --- sys/args ---

    #[test]
    fn test_sys_args_no_vm_context_returns_empty() {
        // Without a VM context set (as in unit test environment), prim_sys_args
        // falls back to an empty list.
        // Note: other tests may set the VM context, so clear it first.
        crate::context::clear_vm_context();
        let (sig, val) = prim_sys_args(&[]);
        assert_eq!(sig, SIG_OK);
        assert!(
            val == Value::EMPTY_LIST,
            "sys/args without trailing args should return empty list, got {:?}",
            val
        );
    }

    #[test]
    fn test_sys_args_reads_from_vm_context() {
        // Set up a VM with user_args and verify prim_sys_args reads them as a list.
        let mut vm = crate::vm::VM::new();
        vm.user_args = vec!["a".to_string(), "b".to_string()];
        crate::context::set_vm_context(&mut vm as *mut crate::vm::VM);

        let (sig, val) = prim_sys_args(&[]);

        crate::context::clear_vm_context();

        assert_eq!(sig, SIG_OK);
        assert!(val.is_list(), "sys/args should return a list");
        let elems = val.list_to_vec().expect("should be a proper list");
        assert_eq!(elems.len(), 2, "expected 2 args");
        assert_eq!(
            elems[0].with_string(|s| s.to_string()),
            Some("a".to_string())
        );
        assert_eq!(
            elems[1].with_string(|s| s.to_string()),
            Some("b".to_string())
        );
    }

    // --- sys/argv ---

    #[test]
    fn test_sys_argv_no_vm_context_returns_empty() {
        crate::context::clear_vm_context();
        let (sig, val) = prim_sys_argv(&[]);
        assert_eq!(sig, SIG_OK);
        assert!(
            val == Value::EMPTY_LIST,
            "sys/argv without VM context should return empty list, got {:?}",
            val
        );
    }

    #[test]
    fn test_sys_argv_reads_source_arg_and_user_args() {
        let mut vm = crate::vm::VM::new();
        vm.source_arg = "-".to_string();
        vm.user_args = vec!["foo".to_string(), "bar".to_string()];
        crate::context::set_vm_context(&mut vm as *mut crate::vm::VM);

        let (sig, val) = prim_sys_argv(&[]);

        crate::context::clear_vm_context();

        assert_eq!(sig, SIG_OK);
        assert!(val.is_list(), "sys/argv should return a list");
        let elems = val.list_to_vec().expect("should be a proper list");
        assert_eq!(elems.len(), 3, "expected 3 elements");
        assert_eq!(
            elems[0].with_string(|s| s.to_string()),
            Some("-".to_string()),
            "element 0 should be source_arg"
        );
        assert_eq!(
            elems[1].with_string(|s| s.to_string()),
            Some("foo".to_string())
        );
        assert_eq!(
            elems[2].with_string(|s| s.to_string()),
            Some("bar".to_string())
        );
    }

    #[test]
    fn test_sys_argv_repl_mode_returns_empty() {
        // In REPL mode source_arg is "", and user_args is empty.
        // sys/argv should return ().
        let mut vm = crate::vm::VM::new();
        vm.source_arg = "".to_string();
        vm.user_args = vec![];
        crate::context::set_vm_context(&mut vm as *mut crate::vm::VM);

        let (sig, val) = prim_sys_argv(&[]);

        crate::context::clear_vm_context();

        assert_eq!(sig, SIG_OK);
        assert!(
            val == Value::EMPTY_LIST,
            "sys/argv in REPL mode should return empty list, got {:?}",
            val
        );
    }

    // --- sys/env ---

    #[test]
    fn test_sys_env_returns_struct() {
        let (sig, val) = prim_sys_env(&[]);
        assert_eq!(sig, SIG_OK);
        assert!(val.as_struct().is_some(), "sys/env should return a struct");
    }

    #[test]
    fn test_sys_env_path_present() {
        let (sig, val) = prim_sys_env(&[]);
        assert_eq!(sig, SIG_OK);
        let fields = val.as_struct().expect("sys/env should return a struct");
        let path_val = crate::value::sorted_struct_get(fields, &TableKey::String("PATH".into()));
        assert!(
            path_val
                .map(|v: &Value| v.with_string(|_| true).unwrap_or(false))
                .unwrap_or(false),
            "sys/env should contain PATH as a string"
        );
    }

    #[test]
    fn test_sys_env_single_lookup_path_is_string() {
        // PATH is always set in a real environment.
        let (sig, val) = prim_sys_env(&[Value::string("PATH")]);
        assert_eq!(sig, SIG_OK);
        assert!(
            val.with_string(|_| true).unwrap_or(false),
            "sys/env with 'PATH' should return a non-nil string"
        );
    }

    #[test]
    fn test_sys_env_single_lookup_unset_returns_nil() {
        let (sig, val) = prim_sys_env(&[Value::string("DEFINITELY_NOT_SET_XYZ_ELLE_123")]);
        assert_eq!(sig, SIG_OK);
        assert!(val.is_nil(), "sys/env with unset var should return nil");
    }

    #[test]
    fn test_sys_env_single_lookup_non_string_type_error() {
        let (sig, _) = prim_sys_env(&[Value::int(42)]);
        assert_eq!(sig, SIG_ERROR);
    }

    // --- subprocess/exec ---

    #[test]
    fn test_subprocess_exec_arity_too_few() {
        let (sig, _) = prim_subprocess_exec(&[]);
        assert_eq!(sig, SIG_ERROR);
        let (sig, _) = prim_subprocess_exec(&[Value::string("echo")]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_subprocess_exec_arity_too_many() {
        let (sig, _) = prim_subprocess_exec(&[
            Value::string("echo"),
            Value::array(vec![]),
            Value::struct_from(std::collections::BTreeMap::new()),
            Value::string("extra"),
        ]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_subprocess_exec_program_not_string() {
        let (sig, _) = prim_subprocess_exec(&[Value::int(42), Value::array(vec![])]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_subprocess_exec_args_not_array() {
        let (sig, _) = prim_subprocess_exec(&[Value::string("echo"), Value::string("not-array")]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_subprocess_exec_args_element_not_string() {
        let (sig, _) =
            prim_subprocess_exec(&[Value::string("echo"), Value::array(vec![Value::int(99)])]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_subprocess_exec_returns_sig_exec_io_yield() {
        let (sig, val) = prim_subprocess_exec(&[
            Value::string("echo"),
            Value::array(vec![Value::string("hi")]),
        ]);
        assert!(sig.contains(SIG_EXEC), "expected SIG_EXEC in {:?}", sig);
        assert!(sig.contains(SIG_IO), "expected SIG_IO in {:?}", sig);
        assert!(sig.contains(SIG_YIELD), "expected SIG_YIELD in {:?}", sig);
        assert_eq!(val.external_type_name(), Some("io-request"));
    }

    // --- extract_string_sequence ---

    #[test]
    fn test_extract_string_sequence_empty_list() {
        let result = extract_string_sequence(&Value::EMPTY_LIST, "test");
        assert_eq!(result, Ok(vec![]));
    }

    #[test]
    fn test_extract_string_sequence_cons_list() {
        let list = Value::cons(
            Value::string("hello"),
            Value::cons(Value::string("world"), Value::EMPTY_LIST),
        );
        let result = extract_string_sequence(&list, "test");
        assert_eq!(result, Ok(vec!["hello".to_string(), "world".to_string()]));
    }

    #[test]
    fn test_extract_string_sequence_array() {
        let arr = Value::array(vec![Value::string("a"), Value::string("b")]);
        let result = extract_string_sequence(&arr, "test");
        assert_eq!(result, Ok(vec!["a".to_string(), "b".to_string()]));
    }

    #[test]
    fn test_extract_string_sequence_array_mut() {
        let arr = Value::array_mut(vec![Value::string("x"), Value::string("y")]);
        let result = extract_string_sequence(&arr, "test");
        assert_eq!(result, Ok(vec!["x".to_string(), "y".to_string()]));
    }

    #[test]
    fn test_extract_string_sequence_type_error() {
        let (sig, _) = extract_string_sequence(&Value::int(42), "test").unwrap_err();
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_extract_string_sequence_non_string_element() {
        let list = Value::cons(Value::int(99), Value::EMPTY_LIST);
        let (sig, _) = extract_string_sequence(&list, "test").unwrap_err();
        assert_eq!(sig, SIG_ERROR);
    }

    // --- subprocess/exec: sequence widening ---

    #[test]
    fn test_subprocess_exec_empty_list_args() {
        let (sig, val) = prim_subprocess_exec(&[Value::string("echo"), Value::EMPTY_LIST]);
        assert!(sig.contains(SIG_EXEC), "expected SIG_EXEC in {:?}", sig);
        assert!(sig.contains(SIG_IO), "expected SIG_IO in {:?}", sig);
        assert!(sig.contains(SIG_YIELD), "expected SIG_YIELD in {:?}", sig);
        assert_eq!(val.external_type_name(), Some("io-request"));
    }

    #[test]
    fn test_subprocess_exec_cons_list_args() {
        let list = Value::cons(
            Value::string("hello"),
            Value::cons(Value::string("world"), Value::EMPTY_LIST),
        );
        let (sig, _) = prim_subprocess_exec(&[Value::string("echo"), list]);
        assert!(sig.contains(SIG_EXEC), "expected SIG_EXEC in {:?}", sig);
    }

    #[test]
    fn test_subprocess_exec_mutable_array_args() {
        let arr = Value::array_mut(vec![Value::string("hi")]);
        let (sig, _) = prim_subprocess_exec(&[Value::string("echo"), arr]);
        assert!(sig.contains(SIG_EXEC), "expected SIG_EXEC in {:?}", sig);
    }

    #[test]
    fn test_subprocess_exec_args_non_sequence_rejected() {
        let (sig, _) = prim_subprocess_exec(&[Value::string("echo"), Value::int(42)]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_subprocess_exec_args_non_string_element_in_list() {
        let list = Value::cons(Value::int(99), Value::EMPTY_LIST);
        let (sig, _) = prim_subprocess_exec(&[Value::string("echo"), list]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_subprocess_exec_improper_list_rejected() {
        let improper = Value::cons(Value::string("a"), Value::int(1));
        let (sig, _) = prim_subprocess_exec(&[Value::string("echo"), improper]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_subprocess_exec_args_non_string_element_in_mutable_array() {
        let arr = Value::array_mut(vec![Value::int(5)]);
        let (sig, _) = prim_subprocess_exec(&[Value::string("echo"), arr]);
        assert_eq!(sig, SIG_ERROR);
    }

    // --- subprocess/wait ---

    #[test]
    fn test_subprocess_wait_arity() {
        let (sig, _) = prim_subprocess_wait(&[]);
        assert_eq!(sig, SIG_ERROR);
        let (sig, _) = prim_subprocess_wait(&[Value::NIL, Value::NIL]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_subprocess_wait_wrong_type() {
        let (sig, _) = prim_subprocess_wait(&[Value::int(42)]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_subprocess_wait_signal_bits() {
        use crate::io::request::ProcessHandle;
        use std::process::Command;
        let child = Command::new("/bin/true").spawn().unwrap();
        let pid = child.id();
        let handle = ProcessHandle::new(pid, child);
        let handle_val = Value::external("process", handle);
        let (sig, val) = prim_subprocess_wait(&[handle_val]);
        assert!(sig.contains(SIG_EXEC), "expected SIG_EXEC in {:?}", sig);
        assert!(sig.contains(SIG_IO), "expected SIG_IO in {:?}", sig);
        assert!(sig.contains(SIG_YIELD));
        assert_eq!(val.external_type_name(), Some("io-request"));
    }

    // --- subprocess/kill ---

    #[test]
    fn test_subprocess_kill_arity() {
        let (sig, _) = prim_subprocess_kill(&[]);
        assert_eq!(sig, SIG_ERROR);
        let (sig, _) = prim_subprocess_kill(&[Value::NIL, Value::NIL, Value::NIL]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_subprocess_kill_wrong_type() {
        // Single int arg — not a process handle or struct, triggers type error
        let (sig, _) = prim_subprocess_kill(&[Value::int(42)]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_subprocess_kill_signal_is_errors() {
        // subprocess/kill is synchronous — it does not yield.
        let def = PRIMITIVES
            .iter()
            .find(|d| d.name == "subprocess/kill")
            .unwrap();
        assert!(!def.signal.may_yield(), "subprocess/kill must not yield");
        assert!(def.signal.may_error());
    }

    #[test]
    fn test_subprocess_kill_keyword_sigterm() {
        use crate::io::request::ProcessHandle;
        use std::process::Command;
        let child = Command::new("/bin/sleep").arg("100").spawn().unwrap();
        let pid = child.id();
        let handle = ProcessHandle::new(pid, child);
        let handle_val = Value::external("process", handle);
        let sig_kw = Value::keyword("sigterm");
        let (sig, _) = prim_subprocess_kill(&[handle_val, sig_kw]);
        assert_eq!(sig, SIG_OK);
    }

    #[test]
    fn test_subprocess_kill_unknown_keyword() {
        use crate::io::request::ProcessHandle;
        use std::process::Command;
        let child = Command::new("/bin/sleep").arg("100").spawn().unwrap();
        let pid = child.id();
        let handle = ProcessHandle::new(pid, child);
        let handle_val = Value::external("process", handle);
        let sig_kw = Value::keyword("sigfoo");
        let (sig, _) = prim_subprocess_kill(&[handle_val, sig_kw]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_subprocess_kill_integer_still_works() {
        use crate::io::request::ProcessHandle;
        use std::process::Command;
        let child = Command::new("/bin/sleep").arg("100").spawn().unwrap();
        let pid = child.id();
        let handle = ProcessHandle::new(pid, child);
        let handle_val = Value::external("process", handle);
        let (sig, _) = prim_subprocess_kill(&[handle_val, Value::int(15)]);
        assert_eq!(sig, SIG_OK);
    }

    // --- subprocess/pid ---

    #[test]
    fn test_subprocess_pid_arity() {
        let (sig, _) = prim_subprocess_pid(&[]);
        assert_eq!(sig, SIG_ERROR);
        let (sig, _) = prim_subprocess_pid(&[Value::NIL, Value::NIL]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_subprocess_pid_wrong_type() {
        let (sig, _) = prim_subprocess_pid(&[Value::int(42)]);
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn test_subprocess_pid_from_handle() {
        use crate::io::request::ProcessHandle;
        use std::process::Command;
        let child = Command::new("/bin/true").spawn().unwrap();
        let expected_pid = child.id();
        let handle = ProcessHandle::new(expected_pid, child);
        let handle_val = Value::external("process", handle);
        let (sig, val) = prim_subprocess_pid(&[handle_val]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(val.as_int(), Some(expected_pid as i64));
    }

    #[test]
    fn test_subprocess_pid_from_struct() {
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
        let (sig, val) = prim_subprocess_pid(&[proc_struct]);
        assert_eq!(sig, SIG_OK);
        assert_eq!(val.as_int(), Some(expected_pid as i64));
    }
}
