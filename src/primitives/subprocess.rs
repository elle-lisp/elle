//! Subprocess-related primitives
use crate::io::request::{IoOp, IoRequest, ProcessHandle, SpawnRequest, StdioDisposition};
use crate::primitives::def::RegionEffect;
use crate::signals::{Signal, SIG_EXEC};
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_HALT, SIG_IO, SIG_OK};
use crate::value::heap::TableKey;
use crate::value::types::Arity;
use crate::value::{sorted_struct_get, Value};

mod exec;
use exec::*;

/// Exit the process with an optional exit code
///
/// (exit)       ; exits with code 0
/// (exit 0)     ; exits with code 0
/// (exit 1)     ; exits with code 1
/// (exit 42)    ; exits with code 42
pub(crate) fn prim_exit(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let code = if args.is_empty() {
        0
    } else if let Some(n) = args[0].as_int() {
        if !(0..=255).contains(&n) {
            return (
                SIG_ERROR,
                ctx.error(
                    "argument-error",
                    format!("exit: code must be between 0 and 255, got {}", n),
                ),
            );
        }
        n as i32
    } else {
        return type_error!(ctx, args[0], "exit", "integer");
    };

    // When the driving VM has the exit trap set (the test runner, around a
    // test's execution), turn `exit` into a catchable signal instead of a
    // process-wide kill, so a single test can't truncate the whole run.
    if ctx.vm().exit_trapped {
        return crate::rich_error!(
            ctx,
            "exited",
            format!("exit {}", code),
            code = Value::int(i64::from(code)),
        );
    }

    std::process::exit(code);
}

/// Enable or disable the driving VM's exit trap (`vm.exit_trapped`). Returns nil.
///
/// (sys/trap-exit! true)   ; `exit` on this VM now emits {:error :exited …}
/// (sys/trap-exit! false)  ; `exit` on this VM terminates the process again
///
/// One VM per worker thread, so this is scoped to the calling OS thread: the test
/// runner brackets each test's execution with it, while the runner's own `exit`
/// (on a VM with the trap unset) still terminates the process normally.
pub(crate) fn prim_trap_exit(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    ctx.vm().exit_trapped = args[0].is_truthy();
    (SIG_OK, Value::NIL)
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
pub(crate) fn prim_halt(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let value = if args.is_empty() { Value::NIL } else { args[0] };
    (SIG_HALT, value)
}

/// Return the current process's pid.
///
/// (sys/pid) => int
pub(crate) fn prim_sys_pid(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
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
pub(crate) fn prim_sys_args(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    let user_args: Vec<Value> = ctx
        .vm()
        .user_args
        .clone()
        .iter()
        .map(|s| ctx.string(s.as_str()))
        .collect();
    (SIG_OK, ctx.list(user_args))
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
pub(crate) fn prim_sys_argv(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    let source_arg = ctx.vm().source_arg.clone();
    if source_arg.is_empty() {
        return (SIG_OK, Value::EMPTY_LIST);
    }
    let user_args = ctx.vm().user_args.clone();
    let mut all: Vec<Value> = Vec::with_capacity(1 + user_args.len());
    all.push(ctx.string(source_arg.as_str()));
    for s in &user_args {
        all.push(ctx.string(s.as_str()));
    }
    (SIG_OK, ctx.list(all))
}

/// Return the process environment as an immutable struct, or look up a single variable.
/// Keys are strings (env var names as-is), values are strings.
/// Non-UTF-8 keys or values are silently skipped.
///
/// (sys/env) => {"HOME" "/home/user" "PATH" "/usr/bin:..." ...}
/// (sys/env "HOME") => "/home/user" or nil if not set
pub(crate) fn prim_sys_env(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args.len() == 1 {
        let name = match args[0].with_string(|s| s.to_string()) {
            Some(s) => s,
            None => {
                return (
                    SIG_ERROR,
                    ctx.error("type-error", "sys/env: expected string argument"),
                )
            }
        };
        return match std::env::var(&name) {
            Ok(val) => (SIG_OK, ctx.string(&*val)),
            Err(_) => (SIG_OK, Value::NIL),
        };
    }
    let mut fields: std::collections::BTreeMap<TableKey, Value> = std::collections::BTreeMap::new();
    for (key, val) in
        std::env::vars_os().filter_map(|(k, v)| k.into_string().ok().zip(v.into_string().ok()))
    {
        fields.insert(TableKey::String(key), ctx.string(val));
    }
    (SIG_OK, ctx.struct_from(fields))
}

/// Parsed subprocess options: (env, cwd, stdin, stdout, stderr).
type ExecOpts = (
    Option<Vec<(String, String)>>,
    Option<String>,
    StdioDisposition,
    StdioDisposition,
    StdioDisposition,
);

primitive! {
    "sys/exit" => prim_exit {
        signal: Signal::halts(),
        arity: Arity::Range(0, 1),
        doc: "Exit the process with an optional exit code (0-255)",
        params: &["code"],
        category: "sys",
        example: "(sys/exit 0)",
        aliases: &["exit", "os/exit"],
        effect: RegionEffect::Immediate,
    }
    "sys/trap-exit!" => prim_trap_exit {
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Trap `exit` on this thread as a catchable {:error :exited :code N} signal (true) or restore process termination (false). Used by the test runner.",
        params: &["on"],
        category: "sys",
        example: "(sys/trap-exit! true)",
        effect: RegionEffect::Immediate,
    }
    "sys/halt" => prim_halt {
        signal: Signal::halts(),
        arity: Arity::Range(0, 1),
        doc: "Halt the VM gracefully, returning a value to the host",
        params: &["value"],
        category: "sys",
        example: "(sys/halt 42)",
        aliases: &["halt", "os/halt"],
        effect: RegionEffect::Immediate,
    }
    "sys/args" => prim_sys_args {
        doc: "Return command-line arguments as a list (excluding interpreter and script path)",
        category: "sys",
        example: "(sys/args)",
        effect: RegionEffect::Fresh,
    }
    "sys/argv" => prim_sys_argv {
        doc: "Return the full argv as a list: script name as element 0 followed by all user args. Element 0 is \"-\" for stdin or the script path for a file. Returns an empty list in REPL mode.",
        category: "sys",
        example: "(sys/argv)",
        effect: RegionEffect::Fresh,
    }
    "sys/pid" => prim_sys_pid {
        arity: Arity::Exact(0),
        doc: "Return the current process's pid as an integer.",
        params: &[],
        category: "sys",
        example: "(sys/pid)",
        aliases: &[],
        effect: RegionEffect::Immediate,
    }
    "sys/env" => prim_sys_env {
        arity: Arity::Range(0, 1),
        doc: "Return the process environment as a struct with string keys and string values, or look up a single variable by name. Non-UTF-8 entries are silently skipped.",
        params: &["name"],
        category: "sys",
        example: "(sys/env) ; or (sys/env \"HOME\")",
        effect: RegionEffect::Fresh,
    }
    "subprocess/exec" => prim_subprocess_exec {
        signal: Signal::subprocess(),
        arity: Arity::Range(2, 3),
        doc: "Spawn a subprocess. Returns {:pid int :stdin port|nil :stdout port|nil :stderr port|nil :process <process>}",
        params: &["program", "args", "opts"],
        category: "sys",
        example: "(subprocess/exec \"ls\" [\"-la\"])",
        // Opaque, not Mixed: copies every heap arg out (program/args/env →
        // Rust String/Vec in the SpawnRequest), storing none, while returning
        // an opaque result minted on the scheduler heap (the {:pid … :process}
        // struct). Mixed would force the multi-heap-arg clique on a no-store
        // primitive — a per-call leak. Pinned by effects.rs
        // `subprocess_exec_declares_opaque_no_arg_clique`. Yielding, so the
        // result side is oracle-exempt. (docs/impl/region/effects.md § Opaque.)
        effect: RegionEffect::Opaque,
    }
    "subprocess/wait" => prim_subprocess_wait {
        signal: Signal::subprocess(),
        arity: Arity::Exact(1),
        doc: "Wait for a subprocess to exit. Returns exit code (0 = success).",
        params: &["handle"],
        category: "sys",
        example: "(subprocess/wait proc)",
        // Immediate: the ProcessWait completion returns the exit code Value::int(..).
        // Yields → oracle-exempt.
        effect: RegionEffect::Immediate,
    }
    "subprocess/kill" => prim_subprocess_kill {
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Send a signal to a subprocess. signal is an integer or a keyword like :sigterm, :sigkill, :sighup, :sigint, :sigquit, :sigpipe, :sigalrm, :sigusr1, :sigusr2, :sigchld, :sigcont, :sigstop, :sigtstp, :sigttin, :sigttou, :sigwinch (default: :sigterm).",
        params: &["handle", "signal"],
        category: "sys",
        example: "(subprocess/kill proc :sigterm)",
        effect: RegionEffect::Immediate,
    }
    "subprocess/pid" => prim_subprocess_pid {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the OS process ID of a subprocess.",
        params: &["handle"],
        category: "sys",
        example: "(subprocess/pid proc)",
        effect: RegionEffect::Immediate,
    }
}

// Tests migrated to tests/elle/prim-subprocess.lisp
