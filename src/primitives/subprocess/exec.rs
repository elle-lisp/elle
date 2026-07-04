use super::*;
use crate::primitives::ctx::NativeCtx;

/// Parse the optional opts struct for subprocess/exec.
/// Returns (env, cwd, stdin, stdout, stderr) or an error tuple.
pub(super) fn parse_exec_opts(
    opts: &Value,
    ctx: &mut NativeCtx,
) -> Result<ExecOpts, (SignalBits, Value)> {
    let fields = match opts.as_struct() {
        Some(f) => f,
        None => {
            return Err((
                SIG_ERROR,
                ctx.error(
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
                        ctx.error("type-error", "subprocess/exec: :env must be a struct"),
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
                            ctx.error(
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
                            ctx.error("type-error", "subprocess/exec: :env values must be strings"),
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
                    ctx.error("type-error", "subprocess/exec: :cwd must be a string"),
                ))
            }
        }),
        None => None,
    };

    // :stdin / :stdout / :stderr — keywords :pipe, :inherit, :null
    fn parse_disp(
        v: &Value,
        field: &str,
        ctx: &mut NativeCtx,
    ) -> Result<StdioDisposition, (SignalBits, Value)> {
        match v.as_keyword_name().as_deref() {
            Some("pipe") => Ok(StdioDisposition::Pipe),
            Some("inherit") => Ok(StdioDisposition::Inherit),
            Some("null") => Ok(StdioDisposition::Null),
            _ => Err((
                SIG_ERROR,
                ctx.error(
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
        Some(v) => parse_disp(v, ":stdin", ctx)?,
        None => StdioDisposition::Pipe,
    };
    let stdout_disp = match sorted_struct_get(fields, &TableKey::Keyword("stdout".into())) {
        Some(v) => parse_disp(v, ":stdout", ctx)?,
        None => StdioDisposition::Pipe,
    };
    let stderr_disp = match sorted_struct_get(fields, &TableKey::Keyword("stderr".into())) {
        Some(v) => parse_disp(v, ":stderr", ctx)?,
        None => StdioDisposition::Pipe,
    };

    Ok((env, cwd, stdin_disp, stdout_disp, stderr_disp))
}

/// Extract a ProcessHandle Value from either:
/// - A Value with external_type_name "process" (direct handle)
/// - A struct with a :process key containing the handle
pub(super) fn extract_process_handle(
    val: &Value,
    fn_name: &str,
    ctx: &mut NativeCtx,
) -> Result<Value, (SignalBits, Value)> {
    if val.external_type_name() == Some("process") {
        return Ok(*val);
    }
    if let Some(fields) = val.as_struct() {
        match sorted_struct_get(fields, &TableKey::Keyword("process".into())) {
            Some(v) => return Ok(*v),
            None => {
                return Err((
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!("{}: struct has no :process key", fn_name),
                    ),
                ))
            }
        }
    }
    Err((
        SIG_ERROR,
        ctx.error(
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
pub(super) fn extract_string_sequence(
    seq: &Value,
    fn_name: &str,
    ctx: &mut NativeCtx,
) -> Result<Vec<String>, (SignalBits, Value)> {
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
                                ctx.error(
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
                        ctx.error(
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
                        ctx.error(
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
                        ctx.error(
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
        ctx.error(
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
pub(super) fn prim_subprocess_exec(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let program = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!(
                        "subprocess/exec: program must be string, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };

    let exec_args = match extract_string_sequence(&args[1], "subprocess/exec", ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let (env, cwd, stdin_disp, stdout_disp, stderr_disp) = if args.len() > 2 {
        match parse_exec_opts(&args[2], ctx) {
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

    let request = IoRequest::portless(
        ctx,
        IoOp::Spawn(SpawnRequest {
            program,
            args: exec_args,
            env,
            cwd,
            stdin: stdin_disp,
            stdout: stdout_disp,
            stderr: stderr_disp,
        }),
    );
    (SIG_YIELD | SIG_IO | SIG_EXEC, request)
}

/// Wait for a subprocess to exit, returning an IoRequest that the scheduler executes.
///
/// (subprocess/wait handle-or-struct) → exit-code
///
/// Returns (SIG_EXEC | SIG_IO | SIG_YIELD, io-request).
pub(super) fn prim_subprocess_wait(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle_val = match extract_process_handle(&args[0], "subprocess/wait", ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };
    // Validate it's actually a ProcessHandle (not just any external)
    if handle_val.as_external::<ProcessHandle>().is_none() {
        return (
            SIG_ERROR,
            ctx.error("type-error", "subprocess/wait: invalid process handle"),
        );
    }
    let request = IoRequest::new(ctx, IoOp::ProcessWait, handle_val);
    (SIG_YIELD | SIG_IO | SIG_EXEC, request)
}

/// Send a signal to a subprocess.
///
/// (subprocess/kill handle-or-struct)           ; sends SIGTERM
/// (subprocess/kill handle-or-struct 15)        ; integer (must round-trip to a named signal)
/// (subprocess/kill handle-or-struct :sigterm)  ; keyword signal name
///
/// Synchronous — returns (SIG_OK, nil) on success, (SIG_ERROR, error) on failure.
pub(super) fn prim_subprocess_kill(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if !(1..=2).contains(&args.len()) {
        return (
            SIG_ERROR,
            ctx.error(
                "argument-error",
                format!(
                    "subprocess/kill: expected 1 or 2 arguments, got {}",
                    args.len()
                ),
            ),
        );
    }
    let handle_val = match extract_process_handle(&args[0], "subprocess/kill", ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let handle = match handle_val.as_external::<ProcessHandle>() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                ctx.error("type-error", "subprocess/kill: invalid process handle"),
            )
        }
    };
    let signal = if args.len() > 1 {
        match crate::io::sigmap::resolve(&args[1], "subprocess/kill") {
            Ok(s) => s,
            Err(e) => {
                let (kind, msg) = e.parts("subprocess/kill");
                return (SIG_ERROR, ctx.error(kind, msg));
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
                ctx.error("exec-error", format!("subprocess/kill: {}", err)),
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
pub(super) fn prim_subprocess_pid(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle_val = match extract_process_handle(&args[0], "subprocess/pid", ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let handle = match handle_val.as_external::<ProcessHandle>() {
        Some(h) => h,
        None => {
            return (
                SIG_ERROR,
                ctx.error("type-error", "subprocess/pid: invalid process handle"),
            )
        }
    };
    (SIG_OK, Value::int(handle.pid() as i64))
}

// Declarative primitive definitions for process operations
