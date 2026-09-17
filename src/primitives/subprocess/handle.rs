//! audited: 2026-09-17
//! The `subprocess` value: the one boundary that checks it, the reads that
//! answer from it, and `wait`/`kill`/`pid`/`exit`.
//!
//! docs/subprocess.md

use super::*;

/// The keys a subprocess answers, in the order `keys` reports them.
///
/// Declaration order rather than sorted: the set is closed and declared here, so
/// the order is ours to pick, and a caller reading `(keys p)` gets the same
/// sequence every run. A struct cannot promise that — its keys come back in
/// `TableKey` hash order.
pub(crate) const KEYS: [&str; 5] = ["pid", "stdin", "stdout", "stderr", "exit"];

/// The handle behind a `subprocess`, or a refusal naming `fn_name`.
///
/// Every primitive that takes a subprocess comes through here, so the check
/// happens once and reads the same however it is reached. There is nothing left
/// to check afterwards: the external either is a `ProcessHandle` or the call
/// already returned.
pub(super) fn extract_subprocess<'v>(
    val: &'v Value,
    fn_name: &str,
    ctx: &mut NativeCtx,
) -> Result<&'v ProcessHandle, (SignalBits, Value)> {
    match val.as_external::<ProcessHandle>() {
        Some(handle) => Ok(handle),
        None => Err((
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!(
                    "{}: expected a subprocess, got {}",
                    fn_name,
                    val.external_type_name().unwrap_or(val.type_name())
                ),
            ),
        )),
    }
}

/// What `handle` answers for `key`, or `None` when it is not one of [`KEYS`].
///
/// The collection primitives reach this rather than carrying a key set of their
/// own, so `get`, `has?`, `keys` and `values` cannot disagree about what a
/// subprocess holds.
pub(crate) fn read(handle: &ProcessHandle, key: Value) -> Option<Value> {
    let which = KEYS.iter().position(|name| Value::keyword(name) == key)?;
    Some(match which {
        0 => Value::int(handle.pid() as i64),
        1 => handle.stdin(),
        2 => handle.stdout(),
        3 => handle.stderr(),
        _ => exit_status(handle),
    })
}

/// The recorded exit status, or nil while nothing has reaped the child. Reads
/// the record; it never asks the kernel, so it cannot take a status the wait
/// that follows is there to report.
fn exit_status(handle: &ProcessHandle) -> Value {
    handle
        .exit()
        .status()
        .map_or(Value::NIL, |code| Value::int(code as i64))
}

/// Wait for a subprocess to exit, returning an IoRequest the scheduler runs.
///
/// (subprocess/wait subprocess) → exit-code
pub(super) fn prim_subprocess_wait(ctx: &mut NativeCtx<'_>, args: &[Value]) -> (SignalBits, Value) {
    if let Err(e) = extract_subprocess(&args[0], "subprocess/wait", ctx) {
        return e;
    }
    let request = IoRequest::new(ctx, IoOp::ProcessWait, args[0]);
    (SIG_IO | SIG_EXEC, request)
}

/// Send a signal to a subprocess.
///
/// (subprocess/kill subprocess)           ; sends SIGTERM
/// (subprocess/kill subprocess 15)        ; integer (must name a known signal)
/// (subprocess/kill subprocess :sigterm)  ; keyword signal name
///
/// Synchronous. `:signaled`, `:exited` or `:missing` — docs/subprocess.md § "Killing
/// a child that may already be gone" says what each reports and why they are three.
pub(super) fn prim_subprocess_kill(ctx: &mut NativeCtx<'_>, args: &[Value]) -> (SignalBits, Value) {
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
    let handle = match extract_subprocess(&args[0], "subprocess/kill", ctx) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let signal = if args.len() > 1 {
        match crate::io::sigmap::resolve(&args[1], "subprocess/kill", ctx.symbols()) {
            Ok(s) => s,
            Err(e) => {
                let (kind, msg) = e.parts("subprocess/kill");
                return (SIG_ERROR, ctx.error(kind, msg));
            }
        }
    } else {
        libc::SIGTERM
    };
    // The handle decides whether there is a child to signal, not the kernel. A
    // reap gives the pid back to the OS, which hands the number out again, so a
    // `kill(2)` on a handle whose record holds a status would reach whoever
    // holds that number now (src/io/AGENTS.md § "A reap is never wasted").
    if handle.exit().status().is_some() {
        return (SIG_OK, ctx.keyword("exited"));
    }
    let ret = unsafe { libc::kill(handle.pid() as i32, signal) };
    if ret < 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ESRCH) {
            (SIG_OK, ctx.keyword("missing"))
        } else {
            (
                SIG_ERROR,
                ctx.error("exec-error", format!("subprocess/kill: {}", err)),
            )
        }
    } else {
        (SIG_OK, ctx.keyword("signaled"))
    }
}

/// The OS process ID of a subprocess, whether or not it has been reaped.
///
/// (subprocess/pid subprocess) → int
pub(super) fn prim_subprocess_pid(ctx: &mut NativeCtx<'_>, args: &[Value]) -> (SignalBits, Value) {
    match extract_subprocess(&args[0], "subprocess/pid", ctx) {
        Ok(handle) => (SIG_OK, Value::int(handle.pid() as i64)),
        Err(e) => e,
    }
}

/// The recorded exit status, or nil while the child runs.
///
/// (subprocess/exit subprocess) → int | nil
pub(super) fn prim_subprocess_exit(ctx: &mut NativeCtx<'_>, args: &[Value]) -> (SignalBits, Value) {
    match extract_subprocess(&args[0], "subprocess/exit", ctx) {
        Ok(handle) => (SIG_OK, exit_status(handle)),
        Err(e) => e,
    }
}

/// (subprocess? value) → boolean. Never errors, matching `port?`.
pub(super) fn prim_is_subprocess(_ctx: &mut NativeCtx<'_>, args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_OK,
        Value::bool(args[0].external_type_name() == Some(SUBPROCESS)),
    )
}
