// audited: 2026-10-04
//! Runs a test's body as the only thread of a fresh process, for the tests that signal, fault or rewire descriptor 0.
//!
//! docs/analysis/testing.md

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(test)]
mod tests;

/// Set in the copy of the test binary that `run` starts. Its value is the
/// name of the test the copy runs.
pub(crate) const COPY: &str = "ELLE_ISOLATE_COPY";

/// Opens the line the copy writes on its stdout. The test's name follows it,
/// then the report.
const REPORT: &str = "elle-isolate:";

/// How long a copy may run beyond its body's deadline: the time to start the
/// test binary and reach the test, on a loaded runner.
const STARTUP: Duration = Duration::from_secs(60);

/// How the body's process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The body returned this code, or passed it to `_exit`.
    Exited(i32),
    /// This signal ended the body's process.
    Signaled(i32),
    /// The body outlived its deadline, and was killed.
    Hung,
}

/// What `run` saw of the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Report {
    /// How the body's process ended.
    pub(crate) end: Outcome,
    /// How many times the body's process stopped. `run` continues a stopped
    /// body each time, so a stop does not end the run.
    pub(crate) stops: u32,
}

impl Report {
    /// The report as the copy writes it after the test's name.
    fn line(&self) -> String {
        let end = match self.end {
            Outcome::Exited(code) => format!("exited {code}"),
            Outcome::Signaled(signum) => format!("signaled {signum}"),
            Outcome::Hung => "hung".to_string(),
        };
        format!("{end} stops {}", self.stops)
    }

    /// The report a `line` wrote, or `None` for any other text.
    fn parse(line: &str) -> Option<Report> {
        let mut words = line.split_whitespace();
        let end = match words.next()? {
            "exited" => Outcome::Exited(words.next()?.parse().ok()?),
            "signaled" => Outcome::Signaled(words.next()?.parse().ok()?),
            "hung" => Outcome::Hung,
            _ => return None,
        };
        if words.next()? != "stops" {
            return None;
        }
        let stops = words.next()?.parse().ok()?;
        words.next().is_none().then_some(Report { end, stops })
    }
}

/// Run `body` as the only thread of a fresh process, and report how that
/// process ended.
///
/// The body's return value is its exit code. A body that is still running when
/// `deadline` passes is killed and reported as `Outcome::Hung`.
///
/// The test's own code before this call runs twice: once in the test process,
/// and once in the copy of the test binary this starts. Call it first.
pub(crate) fn run(deadline: Duration, body: fn() -> i32) -> Report {
    if let Some(name) = std::env::var_os(COPY) {
        report_from_copy(&name.to_string_lossy(), deadline, body);
    }
    let name = std::thread::current()
        .name()
        .map(str::to_string)
        .expect("isolate::run reads the test's name from the thread libtest runs it on");
    let exe = std::env::current_exe().expect("the test binary's path");
    let mut copy = Command::new(exe)
        .args([name.as_str(), "--exact", "--nocapture", "--test-threads=1"])
        .env(COPY, &name)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("start a copy of the test binary to run {name}: {e}"));
    let mut stdout = copy.stdout.take().expect("the copy's stdout is piped");
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });

    let until = Instant::now() + deadline + STARTUP;
    let status = loop {
        match copy.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = copy.kill();
                let _ = copy.wait();
                panic!("the copy running {name} outlived its body's deadline by {STARTUP:?}");
            }
            Err(e) => panic!("wait for the copy running {name}: {e}"),
        }
    };
    let text = reader.join().expect("the copy's stdout reader");
    let opening = format!("{REPORT} {name} ");
    let line = text
        .lines()
        .find_map(|line| line.strip_prefix(&opening))
        .unwrap_or_else(|| {
            panic!(
                "the copy of the test binary ran no test named {name}, or failed before \
                 its body could report ({status}):\n{text}"
            )
        });
    Report::parse(line)
        .unwrap_or_else(|| panic!("the copy running {name} reported {line:?}, which is no report"))
}

/// In the copy: fork the body, wait for it, write its report, and exit.
///
/// The copy holds libtest's main thread, waiting, and this test's thread. No
/// other test runs in it, so no thread holds a lock the fork could copy held.
fn report_from_copy(name: &str, deadline: Duration, body: fn() -> i32) -> ! {
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        panic!("fork failed: {}", std::io::Error::last_os_error());
    }
    if pid == 0 {
        let code = body();
        unsafe { libc::_exit(code) };
    }
    // A line of its own: libtest has written `test NAME ... ` with no newline
    // before running the test.
    let line = format!("\n{REPORT} {name} {}\n", wait_for(pid, deadline).line());
    // Straight to the descriptor: libtest's capture holds what the print
    // macros write, and `_exit` below would discard it.
    unsafe { libc::write(1, line.as_ptr().cast(), line.len()) };
    unsafe { libc::_exit(0) }
}

/// Wait for the body's process `pid` until it ends or `deadline` passes.
/// Continue it each time it stops.
fn wait_for(pid: libc::pid_t, deadline: Duration) -> Report {
    let until = Instant::now() + deadline;
    let mut stops = 0;
    let mut status: libc::c_int = 0;
    loop {
        let reaped = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG | libc::WUNTRACED) };
        if reaped < 0 {
            panic!("waitpid({pid}): {}", std::io::Error::last_os_error());
        }
        if reaped == pid {
            if libc::WIFSTOPPED(status) {
                stops += 1;
                unsafe { libc::kill(pid, libc::SIGCONT) };
                continue;
            }
            let end = if libc::WIFSIGNALED(status) {
                Outcome::Signaled(libc::WTERMSIG(status))
            } else {
                Outcome::Exited(libc::WEXITSTATUS(status))
            };
            return Report { end, stops };
        }
        if Instant::now() >= until {
            unsafe { libc::kill(pid, libc::SIGKILL) };
            unsafe { libc::waitpid(pid, &mut status, 0) };
            return Report {
                end: Outcome::Hung,
                stops,
            };
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
