//! audited: 2026-09-30
//! The `subprocess` a spawn answers with — its pid, its stdio ports and its
//! exit record — and the record, with what the child cost.
//!
//! docs/subprocess.md

use crate::value::Value;
use std::cell::RefCell;
use std::process::Child;
use std::sync::{Arc, Mutex};

/// The external type name a spawned child is stored under.
///
/// One constant rather than the literal at each site: the spawn that mints the
/// value, the extractor that checks it, `subprocess?`, and the display dispatch
/// all have to agree, and a typo in any of them is a value nothing recognizes.
/// `process` is deliberately not the word — that one already means an
/// Erlang-style process here (docs/processes.md).
pub(crate) const SUBPROCESS: &str = "subprocess";

/// A spawned child. Stored as an external named [`SUBPROCESS`], and the only
/// shape `subprocess/wait`, `subprocess/kill`, `subprocess/pid` and
/// `subprocess/exit` take.
#[derive(Debug)]
pub(crate) struct ProcessHandle {
    pid: u32,
    /// The spawned child, kept so an unreaped one can be reaped on drop. The
    /// exit status is NOT read back through it: a wait reaps with `wait4(2)` on
    /// the pid, which leaves this `Child` believing the process is still
    /// running.
    child: RefCell<Child>,
    exit: ExitRecord,
    /// The child's stdio ports, or `Value::NIL` where the disposition asked for
    /// no pipe. Read by `get`/`keys`/`values`, never written after the spawn.
    ///
    /// These are heap `Value`s an external holds, which no alloc-time scan and
    /// no free-time cascade enumerates (docs/impl/region/rules.md Rule 5). They
    /// need no count of their own because `spawn_to_subprocess` mints them and
    /// this handle through ONE `Alloc`: they share a region, so nothing can free
    /// a port while the subprocess holding it is alive.
    stdio: [Value; 3],
}

impl ProcessHandle {
    /// A handle over a child with no stdio ports — every spawn adds them with
    /// [`with_stdio`](Self::with_stdio).
    pub fn new(pid: u32, child: Child) -> Self {
        ProcessHandle {
            pid,
            child: RefCell::new(child),
            exit: ExitRecord::new(),
            stdio: [Value::NIL; 3],
        }
    }

    /// The same handle, carrying the ports the spawn created. Each is a port
    /// `Value` or `Value::NIL`, in stdin/stdout/stderr order.
    pub(crate) fn with_stdio(mut self, stdio: [Value; 3]) -> Self {
        self.stdio = stdio;
        self
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// The child's stdin port, or nil.
    pub(crate) fn stdin(&self) -> Value {
        self.stdio[0]
    }

    /// The child's stdout port, or nil.
    pub(crate) fn stdout(&self) -> Value {
        self.stdio[1]
    }

    /// The child's stderr port, or nil.
    pub(crate) fn stderr(&self) -> Value {
        self.stdio[2]
    }

    /// Where this child's exit status is kept. Every operation that may reap
    /// the child carries a clone; see src/io/AGENTS.md § "A reap is never
    /// wasted".
    pub(crate) fn exit(&self) -> &ExitRecord {
        &self.exit
    }
}

/// `#<subprocess 12345>` — the pid is what a reader needs and the rest is
/// unprintable state.
impl std::fmt::Display for ProcessHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#<subprocess {}>", self.pid)
    }
}

/// Reap the subprocess on drop to prevent zombie accumulation.
/// `try_wait` is non-blocking; if the process hasn't exited yet,
/// it stays in the OS process table until it does.
impl Drop for ProcessHandle {
    fn drop(&mut self) {
        if self.exit.status().is_none() {
            let _ = self.child.borrow_mut().try_wait();
        }
    }
}

/// Where a child's exit status is kept once somebody has reaped it, with what
/// the child cost.
///
/// A reap consumes the status: the kernel hands it over once, and the child is
/// then gone. So the status cannot travel only in the completion of the
/// operation that took it — a wait a deadline cancelled reaps just as
/// effectively as one that answers, and its completion reaches nobody. See
/// src/io/AGENTS.md § "A reap is never wasted" for the argument. The usage the
/// kernel reports with the status is gone after the reap too, so it is kept
/// beside it.
///
/// Shared, and shared across threads: a pool worker reaps on its own thread,
/// and the ring's completion reaps on the scheduler's. The pending entry and
/// the pool operation each hold a clone, so recording reaches no heap value and
/// stays sound on a teardown drain.
#[derive(Debug, Clone)]
pub(crate) struct ExitRecord(Arc<Mutex<Option<Exit>>>);

/// What a reap left behind.
#[derive(Debug, Clone, Copy)]
struct Exit {
    code: i32,
    /// `None` only for a status a test planted without a reap.
    usage: Option<Usage>,
}

/// What one ask of the kernel produced.
pub(crate) enum Reap {
    /// The child's exit status: this ask reaped it, or the record was already
    /// holding it. Both are an answer, and a waiter cannot tell them apart.
    Exited(i32),
    /// The child is still running. Ask again later.
    Running,
    /// The ask failed, with this errno.
    Failed(i32),
}

impl ExitRecord {
    /// A record for a child nobody has reaped.
    pub(crate) fn new() -> ExitRecord {
        ExitRecord(Arc::new(Mutex::new(None)))
    }

    /// The status this process is holding for the child, if any.
    pub(crate) fn status(&self) -> Option<i32> {
        self.held().map(|exit| exit.code)
    }

    /// Plant a status no reap produced: the state a finished wait leaves, for
    /// a test that cannot make the kernel produce it on demand. The first
    /// status wins, as it does for a reap.
    #[cfg(test)]
    pub(crate) fn keep(&self, code: i32) {
        let mut held = self.held();
        if held.is_none() {
            *held = Some(Exit { code, usage: None });
        }
    }

    /// Ask the kernel for `pid`'s status once, and keep whatever comes back,
    /// with the usage it reports.
    ///
    /// The record is held across the `wait4` call, which is what makes the ask
    /// and the record one step. Two waits on one child are legal, and the
    /// loser's `wait4` finds a child that is gone; a record consulted after the
    /// syscall would leave the loser reading in the gap between the winner's
    /// reap and the winner's write, and reporting `ECHILD` for a status this
    /// process is holding.
    ///
    /// `WNOHANG` rather than a blocking wait: the kernel reports no readiness
    /// for a child that has not exited, so the caller paces its asks with the
    /// stop pipe visible between them (`src/io/threadpool/child.rs`), or asks
    /// only once the ring has said the child exited.
    pub(crate) fn reap(&self, pid: u32) -> Reap {
        let mut held = self.held();
        if let Some(exit) = *held {
            return Reap::Exited(exit.code);
        }
        loop {
            let mut status: libc::c_int = 0;
            // SAFETY: an all-zero rusage is a valid value to hand the kernel.
            let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
            // SAFETY: both out-pointers are live and writable for the call.
            let ret =
                unsafe { libc::wait4(pid as libc::pid_t, &mut status, libc::WNOHANG, &mut ru) };
            if ret > 0 {
                let code = exit_code_from_wait_status(status);
                *held = Some(Exit {
                    code,
                    usage: Some(Usage::from_rusage(&ru)),
                });
                return Reap::Exited(code);
            }
            if ret == 0 {
                return Reap::Running;
            }
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(1);
            if errno == libc::EINTR {
                continue;
            }
            return Reap::Failed(errno);
        }
    }

    /// What the child has cost: the usage the reap kept, or a live sample of
    /// `pid` while nothing has reaped it (docs/subprocess.md).
    ///
    /// The sample is taken under the record's lock. A reap needs the same
    /// lock, so the pid cannot be reaped, handed back to the kernel and given
    /// to another process between the check and the read.
    pub(crate) fn usage(&self, pid: u32) -> Option<Usage> {
        let held = self.held();
        match *held {
            Some(exit) => exit.usage,
            None => Usage::sample(pid),
        }
    }

    /// The exit under the lock. A poisoned mutex still holds a readable
    /// value — a panic elsewhere cannot leave a half-written exit — so the
    /// guard is taken back rather than propagated.
    fn held(&self) -> std::sync::MutexGuard<'_, Option<Exit>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The exit code in a `wait4(2)` status word. A signalled child reports the
/// signal number negated, which is what `subprocess/wait` answers with.
pub(crate) fn exit_code_from_wait_status(status: libc::c_int) -> i32 {
    if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status)
    } else if libc::WIFSIGNALED(status) {
        -libc::WTERMSIG(status)
    } else {
        -1
    }
}

/// What a child cost: CPU time in user mode and in the kernel, in
/// microseconds, and the peak resident set in KiB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Usage {
    pub user_us: i64,
    pub sys_us: i64,
    pub max_rss_kb: i64,
}

impl Usage {
    /// The usage a reap reported: the child and every descendant it waited
    /// for. Linux counts `ru_maxrss` in KiB and macOS in bytes.
    ///
    /// `time_t`, `suseconds_t` and `c_long` differ in width across targets, so
    /// a widening that is a no-op on one target is not on another.
    #[allow(clippy::useless_conversion)]
    fn from_rusage(ru: &libc::rusage) -> Usage {
        let us = |tv: libc::timeval| i64::from(tv.tv_sec) * 1_000_000 + i64::from(tv.tv_usec);
        let rss = i64::from(ru.ru_maxrss);
        Usage {
            user_us: us(ru.ru_utime),
            sys_us: us(ru.ru_stime),
            max_rss_kb: if cfg!(target_os = "macos") {
                rss / 1024
            } else {
                rss
            },
        }
    }

    /// A running child's usage so far, from `/proc`. `None` once the child has
    /// exited: its memory is gone, and `status` no longer carries `VmHWM`.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn sample(pid: u32) -> Option<Usage> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        // SAFETY: sysconf reads a constant and has no preconditions.
        let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as i64;
        if hz <= 0 {
            return None;
        }
        // The command name is field 2, in parentheses, and may itself hold
        // spaces and parentheses. The fields after the last `)` start at
        // field 3, so utime and stime — fields 14 and 15 — are the 12th and
        // 13th of them.
        let mut fields = stat[stat.rfind(')')? + 1..].split_whitespace();
        let utime: i64 = fields.nth(11)?.parse().ok()?;
        let stime: i64 = fields.next()?.parse().ok()?;
        let hwm = status
            .lines()
            .find_map(|l| l.strip_prefix("VmHWM:"))?
            .split_whitespace()
            .next()?
            .parse()
            .ok()?;
        Some(Usage {
            user_us: utime * 1_000_000 / hz,
            sys_us: stime * 1_000_000 / hz,
            max_rss_kb: hwm,
        })
    }

    /// A running child's usage so far, from `proc_pid_rusage`. macOS keeps no
    /// peak for another process, so the resident set is the one at the call.
    #[cfg(target_os = "macos")]
    fn sample(pid: u32) -> Option<Usage> {
        // SAFETY: an all-zero rusage_info_v2 is a valid out-buffer.
        let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
        // SAFETY: `info` is the buffer RUSAGE_INFO_V2 names, live for the call.
        let ret = unsafe {
            libc::proc_pid_rusage(
                pid as libc::c_int,
                libc::RUSAGE_INFO_V2,
                &mut info as *mut libc::rusage_info_v2 as *mut libc::rusage_info_t,
            )
        };
        if ret != 0 {
            return None;
        }
        Some(Usage {
            user_us: (mach_ns(info.ri_user_time) / 1000) as i64,
            sys_us: (mach_ns(info.ri_system_time) / 1000) as i64,
            max_rss_kb: (info.ri_resident_size / 1024) as i64,
        })
    }

    /// No way to read another process's usage here.
    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
    fn sample(_pid: u32) -> Option<Usage> {
        None
    }
}

/// Mach absolute time in nanoseconds. `proc_pid_rusage` reports CPU time in
/// Mach ticks, which are nanoseconds on Intel and are not on Apple silicon.
#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn mach_ns(ticks: u64) -> u64 {
    let mut tb = libc::mach_timebase_info { numer: 0, denom: 0 };
    // SAFETY: `tb` is a live, writable timebase struct.
    if unsafe { libc::mach_timebase_info(&mut tb) } != 0 || tb.denom == 0 {
        return ticks;
    }
    (ticks as u128 * tb.numer as u128 / tb.denom as u128) as u64
}

/// A child that has exited and has already been reaped.
///
/// `ProcessHandle::new` demands a `Child`, so a test that builds a handle over
/// a pid of its own choosing still has to supply one. This stand-in is already
/// reaped, so it leaves no zombie and its `Drop` has nothing to do.
#[cfg(test)]
pub(crate) fn reaped_child() -> Child {
    // Resolved through `PATH`, not hardcoded: no absolute path is right
    // everywhere. macOS ships no `/bin/true`, and a busybox image ships no
    // `/usr/bin/true`.
    let mut child = std::process::Command::new("true").spawn().unwrap();
    child.wait().unwrap();
    child
}

/// A child that has exited and is still waiting to be reaped.
///
/// The trap the tests here depend on: `waitpid` cannot leave a status in place.
/// `WNOWAIT` is a `waitid(2)` flag, and `wait4(2)` — which is what `waitpid`
/// becomes on Linux — rejects it with `EINVAL`. So the ask below is a `waitid`,
/// and it blocks until the child has exited while leaving the status for the
/// real reap the test is about to make.
///
/// `false` rather than `true`, so a status read back as `0` cannot be a default
/// that nothing wrote.
#[cfg(test)]
pub(crate) fn zombie_child() -> Child {
    let child = std::process::Command::new("false").spawn().unwrap();
    let pid = child.id() as libc::id_t;
    // SAFETY: `infop` is a live, writable siginfo_t for the duration of the call.
    let ret = unsafe {
        let mut info: libc::siginfo_t = std::mem::zeroed();
        libc::waitid(libc::P_PID, pid, &mut info, libc::WEXITED | libc::WNOWAIT)
    };
    assert_eq!(
        ret,
        0,
        "waitid(WNOWAIT) must leave the child reapable: {}",
        std::io::Error::last_os_error(),
    );
    child
}
