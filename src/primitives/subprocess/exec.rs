//! audited: 2026-09-17
//! `subprocess/exec`: the options it parses, and the sequence widening its
//! args take.
//!
//! docs/subprocess.md

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
    let env = match sorted_struct_get(fields, &TableKey::keyword("env")) {
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
                let key_str =
                    match k {
                        TableKey::Keyword(hash) => {
                            match ctx.keyword_spelling(Value::keyword_from_hash(*hash)) {
                                Some(s) => s,
                                None => return Err((
                                    SIG_ERROR,
                                    ctx.error(
                                        "type-error",
                                        "subprocess/exec: :env keyword key has no learned spelling",
                                    ),
                                )),
                            }
                        }
                        TableKey::String(v) => {
                            v.as_str().expect("a string key holds a string").to_string()
                        }
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
    let cwd = match sorted_struct_get(fields, &TableKey::keyword("cwd")) {
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
        match ctx.keyword_spelling(*v).as_deref() {
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

    let stdin_disp = match sorted_struct_get(fields, &TableKey::keyword("stdin")) {
        Some(v) => parse_disp(v, ":stdin", ctx)?,
        None => StdioDisposition::Pipe,
    };
    let stdout_disp = match sorted_struct_get(fields, &TableKey::keyword("stdout")) {
        Some(v) => parse_disp(v, ":stdout", ctx)?,
        None => StdioDisposition::Pipe,
    };
    let stderr_disp = match sorted_struct_get(fields, &TableKey::keyword("stderr")) {
        Some(v) => parse_disp(v, ":stderr", ctx)?,
        None => StdioDisposition::Pipe,
    };

    Ok((env, cwd, stdin_disp, stdout_disp, stderr_disp))
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
/// Answers `(SIG_IO | SIG_EXEC, io-request)`. The yield is the scheduler's:
/// `SIG_IO` is what routes the request to a backend and parks the fiber.
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
    (SIG_IO | SIG_EXEC, request)
}
