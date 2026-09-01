use super::*;

/// How to configure a subprocess stdio stream.
#[derive(Debug, Clone, Copy)]
pub enum StdioDisposition {
    /// Create a pipe; return it as a port.
    Pipe,
    /// Inherit the parent process fd.
    Inherit,
    /// Redirect to /dev/null.
    Null,
}

/// Subprocess configuration, shared between IoOp::Spawn and the backend helpers.
#[derive(Debug)]
pub struct SpawnRequest {
    pub program: String,
    pub args: Vec<String>,
    pub env: Option<Vec<(String, String)>>,
    pub cwd: Option<String>,
    pub stdin: StdioDisposition,
    pub stdout: StdioDisposition,
    pub stderr: StdioDisposition,
}

impl StdioDisposition {
    fn to_std(self) -> std::process::Stdio {
        match self {
            StdioDisposition::Pipe => std::process::Stdio::piped(),
            StdioDisposition::Inherit => std::process::Stdio::inherit(),
            StdioDisposition::Null => std::process::Stdio::null(),
        }
    }
}

/// Reset the child's signal state after `fork(2)`, before `execve(2)`.
///
/// Elle blocks signals on every thread it spawns internally (the I/O thread
/// pool, the stdin reader, the JIT worker, and the `os/spawn` worker the test
/// runner runs whole-file thunks in) and the absorb set on the main thread —
/// all for its `signalfd` machinery (see docs/posix-signals.md). `fork` copies
/// the spawning thread's mask into the child and `execve` *preserves* it, so a
/// child spawned from a masked thread would start with signals blocked: `sleep`
/// would ignore `subprocess/kill … 15` (SIGTERM) and only die to the
/// unblockable SIGKILL, wedging `subprocess/wait`. Reset to the state a shell
/// hands its children — empty mask, default SIGPIPE.
///
/// Runs in the forked child (single-threaded at that point) and uses only
/// async-signal-safe libc calls (`sigprocmask`/`sigemptyset`/`sigaction`).
#[cfg(unix)]
pub(super) fn reset_child_signals() -> std::io::Result<()> {
    // SAFETY: post-fork, pre-exec, single-threaded child; every call here is on
    // the POSIX async-signal-safe list. `sigset_t`/`sigaction` zeroed is a valid
    // initialized state for the libc calls that follow.
    unsafe {
        // 1. Unblock everything — undo elle's internal masking for the child.
        let mut empty: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut empty);
        libc::sigprocmask(libc::SIG_SETMASK, &empty, std::ptr::null_mut());

        // 2. Restore SIGPIPE to SIG_DFL. `execve` resets *caught* handlers to
        //    default automatically, but a SIG_IGN disposition survives exec, and
        //    elle sets SIGPIPE to SIG_IGN process-wide. Without this a child that
        //    relies on the default "die on a broken pipe" (e.g. `head` closing a
        //    pipeline early) would instead see `write` fail with EPIPE.
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = libc::SIG_DFL;
        libc::sigemptyset(&mut sa.sa_mask);
        libc::sigaction(libc::SIGPIPE, &sa, std::ptr::null_mut());
    }
    Ok(())
}

impl SpawnRequest {
    /// Build a `std::process::Command` from this request.
    pub(crate) fn build_command(&self) -> std::process::Command {
        let mut cmd = std::process::Command::new(&self.program);
        cmd.args(&self.args);
        if let Some(ref env_pairs) = self.env {
            cmd.env_clear();
            for (k, v) in env_pairs {
                cmd.env(k, v);
            }
        }
        if let Some(ref dir) = self.cwd {
            cmd.current_dir(dir);
        }
        cmd.stdin(self.stdin.to_std());
        cmd.stdout(self.stdout.to_std());
        cmd.stderr(self.stderr.to_std());
        // Give the child a clean signal slate (empty mask, default SIGPIPE) so
        // elle's internal masking never leaks across exec. See
        // `reset_child_signals`.
        #[cfg(unix)]
        // SAFETY: `pre_exec` runs the closure in the forked child between fork
        // and exec; `reset_child_signals` only calls async-signal-safe libc.
        unsafe {
            use std::os::unix::process::CommandExt;
            cmd.pre_exec(reset_child_signals);
        }
        cmd
    }

    /// Spawn the subprocess and convert it to an Elle struct value.
    ///
    /// Returns `Ok(struct)` with `:pid`, `:stdin`, `:stdout`, `:stderr`,
    /// `:process` fields, or `Err(error_val)` on failure.
    pub(crate) fn spawn_to_struct(
        &self,
        origin_heap: *mut crate::value::fiberheap::FiberHeap,
    ) -> Result<Value, Value> {
        use crate::value::heap::TableKey;

        let mut child = self.build_command().spawn().map_err(|e| {
            crate::io::io_error(
                "exec-error",
                format!("subprocess/exec: {}: {e}", self.program),
                origin_heap,
            )
        })?;

        let pid = child.id();
        // One allocation capability over the requesting instance's heap: the pipe
        // ports, the process handle, and the wrapper struct share its region and
        // live on the heap the receiving fiber manages (no cross-heap references).
        let heap = unsafe { &mut *crate::io::completion_heap_ptr(origin_heap) };
        let ctx = crate::primitives::ctx::Alloc::new(heap);

        let stdin_val = child
            .stdin
            .take()
            .map(|s| pipe_to_port(&ctx, s, Direction::Write, Encoding::Binary, pid, "stdin"))
            .unwrap_or(Value::NIL);
        let stdout_val = child
            .stdout
            .take()
            .map(|s| pipe_to_port(&ctx, s, Direction::Read, Encoding::Binary, pid, "stdout"))
            .unwrap_or(Value::NIL);
        let stderr_val = child
            .stderr
            .take()
            .map(|s| pipe_to_port(&ctx, s, Direction::Read, Encoding::Binary, pid, "stderr"))
            .unwrap_or(Value::NIL);

        let handle = ProcessHandle::new(pid, child);
        let handle_val = ctx.external("process", handle);

        let mut fields = std::collections::BTreeMap::new();
        fields.insert(TableKey::keyword("pid"), Value::int(pid as i64));
        fields.insert(TableKey::keyword("stdin"), stdin_val);
        fields.insert(TableKey::keyword("stdout"), stdout_val);
        fields.insert(TableKey::keyword("stderr"), stderr_val);
        fields.insert(TableKey::keyword("process"), handle_val);
        Ok(ctx.struct_from(fields))
    }
}

/// Convert a subprocess pipe (stdin/stdout/stderr) to a Port value, born in
/// `ctx`'s region (the requesting instance's heap — see `spawn_to_struct`).
pub(super) fn pipe_to_port<T: Into<std::os::unix::io::OwnedFd>>(
    ctx: &crate::primitives::ctx::Alloc,
    pipe: T,
    direction: Direction,
    encoding: Encoding,
    pid: u32,
    name: &str,
) -> Value {
    let fd: std::os::unix::io::OwnedFd = pipe.into();
    let label = format!("pid:{}:{}", pid, name);
    ctx.external("port", Port::new_pipe(fd, direction, encoding, label))
}
