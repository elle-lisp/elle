//! Subprocess-related primitives
use crate::io::request::{IoOp, IoRequest, ProcessHandle, SpawnRequest, StdioDisposition};
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
    } else if let Some(n) = args[0].as_int() {
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
    let value = if args.is_empty() { Value::NIL } else { args[0] };
    (SIG_HALT, value)
}

/// Return the current process's pid.
///
/// (sys/pid) => int
pub(crate) fn prim_sys_pid(_args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::int(std::process::id() as i64))
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

    // Pair list (proper only)
    if seq.as_pair().is_some() {
        let mut current = *seq;
        loop {
            if current.is_empty_list() {
                break;
            }
            match current.as_pair() {
                Some(pair) => {
                    match pair.first.with_string(|s| s.to_string()) {
                        Some(s) => result.push(s),
                        None => {
                            return Err((
                                SIG_ERROR,
                                error_val(
                                    "type-error",
                                    format!(
                                        "{}: args element must be string, got {}",
                                        fn_name,
                                        pair.first.type_name()
                                    ),
                                ),
                            ))
                        }
                    }
                    current = pair.rest;
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

/// Send a signal to a subprocess.
///
/// (subprocess/kill handle-or-struct)           ; sends SIGTERM
/// (subprocess/kill handle-or-struct 15)        ; integer (must round-trip to a named signal)
/// (subprocess/kill handle-or-struct :sigterm)  ; keyword signal name
///
/// Synchronous — returns (SIG_OK, nil) on success, (SIG_ERROR, error) on failure.
fn prim_subprocess_kill(args: &[Value]) -> (SignalBits, Value) {
    if !(1..=2).contains(&args.len()) {
        return (
            SIG_ERROR,
            error_val(
                "argument-error",
                format!(
                    "subprocess/kill: expected 1 or 2 arguments, got {}",
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
        match crate::io::sigmap::resolve(&args[1], "subprocess/kill") {
            Ok(s) => s,
            Err(e) => {
                let (kind, msg) = e.parts("subprocess/kill");
                return (SIG_ERROR, error_val(kind, msg));
            }
        }
    } else {
        libc::SIGTERM
    };
    let ret = unsafe { libc::kill(handle.pid() as i32, signal) };
    if ret < 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ESRCH) {
            // Process already exited — treat as success
            (SIG_OK, Value::NIL)
        } else {
            (
                SIG_ERROR,
                error_val("exec-error", format!("subprocess/kill: {}", err)),
            )
        }
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

// Declarative primitive definitions for process operations
primitive! {
    "sys/exit" => prim_exit {
        signal: Signal::halts(),
        arity: Arity::Range(0, 1),
        doc: "Exit the process with an optional exit code (0-255)",
        params: &["code"],
        category: "sys",
        example: "(sys/exit 0)",
        aliases: &["exit", "os/exit"],
    }
    "sys/halt" => prim_halt {
        signal: Signal::halts(),
        arity: Arity::Range(0, 1),
        doc: "Halt the VM gracefully, returning a value to the host",
        params: &["value"],
        category: "sys",
        example: "(sys/halt 42)",
        aliases: &["halt", "os/halt"],
    }
    "sys/args" => prim_sys_args {
        doc: "Return command-line arguments as a list (excluding interpreter and script path)",
        category: "sys",
        example: "(sys/args)",
    }
    "sys/argv" => prim_sys_argv {
        doc: "Return the full argv as a list: script name as element 0 followed by all user args. Element 0 is \"-\" for stdin or the script path for a file. Returns an empty list in REPL mode.",
        category: "sys",
        example: "(sys/argv)",
    }
    "sys/pid" => prim_sys_pid {
        arity: Arity::Exact(0),
        doc: "Return the current process's pid as an integer.",
        params: &[],
        category: "sys",
        example: "(sys/pid)",
        aliases: &[],
    }
    "sys/env" => prim_sys_env {
        arity: Arity::Range(0, 1),
        doc: "Return the process environment as a struct with string keys and string values, or look up a single variable by name. Non-UTF-8 entries are silently skipped.",
        params: &["name"],
        category: "sys",
        example: "(sys/env) ; or (sys/env \"HOME\")",
    }
    "subprocess/exec" => prim_subprocess_exec {
        signal: (Signal {
            // SIG_EXEC: capability bit for fiber mask access control.
            // SIG_IO: dispatch bit — routes through the I/O scheduler.
            // Both are emitted; dispatch is IO-based; exec bit enables capability gating.
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO).union(SIG_EXEC),
            propagates: 0,
        }),
        arity: Arity::Range(2, 3),
        doc: "Spawn a subprocess. Returns {:pid int :stdin port|nil :stdout port|nil :stderr port|nil :process <process>}",
        params: &["program", "args", "opts"],
        category: "sys",
        example: "(subprocess/exec \"ls\" [\"-la\"])",
    }
    "subprocess/wait" => prim_subprocess_wait {
        signal: (Signal {
            // SIG_EXEC: capability bit (same fiber mask semantics as subprocess/exec).
            bits: SIG_ERROR.union(SIG_YIELD).union(SIG_IO).union(SIG_EXEC),
            propagates: 0,
        }),
        arity: Arity::Exact(1),
        doc: "Wait for a subprocess to exit. Returns exit code (0 = success).",
        params: &["handle"],
        category: "sys",
        example: "(subprocess/wait proc)",
    }
    "subprocess/kill" => prim_subprocess_kill {
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Send a signal to a subprocess. signal is an integer or a keyword like :sigterm, :sigkill, :sighup, :sigint, :sigquit, :sigpipe, :sigalrm, :sigusr1, :sigusr2, :sigchld, :sigcont, :sigstop, :sigtstp, :sigttin, :sigttou, :sigwinch (default: :sigterm).",
        params: &["handle", "signal"],
        category: "sys",
        example: "(subprocess/kill proc :sigterm)",
    }
    "subprocess/pid" => prim_subprocess_pid {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the OS process ID of a subprocess.",
        params: &["handle"],
        category: "sys",
        example: "(subprocess/pid proc)",
    }
}

// Tests migrated to tests/elle/prim-subprocess.lisp
