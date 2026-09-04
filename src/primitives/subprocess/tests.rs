//! `subprocess/kill` against the handle's exit record.

use super::*;
use crate::io::request::{reaped_child, Reap};
use crate::primitives::ctx::TestHeap;
use std::process::{Child, Command};
use std::time::Duration;

/// A process that outlives the test around it, so "still running" means
/// nothing signalled it rather than "the test was quick enough".
fn long_lived_child() -> Child {
    Command::new("sleep").arg("30").spawn().unwrap()
}

/// Send `signal` through the primitive.
///
/// The trap: the answer is compared as a value rather than as a spelling. A
/// keyword IS its name hash (docs/impl/symbol.md), and the spelling lives in a
/// symbol table a bare `TestHeap` VM does not carry — so `keyword_spelling`
/// here answers `None` for a keyword the primitive minted correctly.
fn kill(h: &TestHeap, handle: Value, signal: &str) -> (SignalBits, Value) {
    let mut ctx = h.ctx();
    let signal = ctx.keyword(signal);
    prim_subprocess_kill(&mut ctx, &[handle, signal])
}

/// A kill on a handle whose record holds a status makes no `kill(2)` at all.
///
/// The trap: a pid cannot be recycled on demand. The kernel decides who gets a
/// freed number, and waiting for it to pick this one is not a test. So the
/// handle is built over a pid that already belongs to somebody else, which puts
/// the kill in front of the same choice with a known victim.
///
/// The counter-factual is `ESRCH`, which the primitive reports as success: on a
/// quiet machine a reaped child's number is free, so signalling it looks fine
/// and a test that kills a reaped child of its own passes either way. `SIGKILL`
/// is what makes the victim's side readable — it cannot be caught or blocked,
/// so a process still running is one that was never signalled.
#[test]
fn a_kill_on_a_reaped_child_sends_no_signal() {
    crate::value::arena::with_test_region(|| {
        let mut victim = long_lived_child();
        let pid = victim.id();

        let h = TestHeap::new();
        let handle = ProcessHandle::new(pid, reaped_child());
        // The state a `subprocess/wait` leaves behind: the child this handle
        // was spawned for is gone, and its status is here.
        handle.exit().keep(0);
        let handle_val = h.ctx().external("process", handle);

        let (bits, answer) = kill(&h, handle_val, "sigkill");

        // The syscall first: this is what the answer below is only a report of,
        // and it fails on its own.
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            matches!(victim.try_wait(), Ok(None)),
            "the pid's current owner was signalled, so the kill reached the kernel"
        );

        assert_eq!(bits, SIG_OK, "a child that is already gone is not an error");
        assert_eq!(
            answer,
            Value::keyword("exited"),
            "the answer must say the child had exited"
        );

        victim.kill().unwrap();
        victim.wait().unwrap();
    });
}

/// A kill that finds nobody holding the pid says that, rather than claiming the
/// child exited.
///
/// The trap: `ESRCH` is the one answer the kernel gives about a number rather
/// than about a process. A pid keeps no record of who used to hold it, so
/// `ESRCH` cannot establish that the process it names was ever this handle's
/// child — which is why it is not folded into `:exited`, whose evidence is the
/// handle's own status.
///
/// The record is left empty on purpose: with a status in it the call answers
/// from the record and never reaches the syscall this test is about.
#[test]
fn a_kill_on_a_pid_nobody_holds_reports_it_missing() {
    crate::value::arena::with_test_region(|| {
        // Reaped before the handle is built, so the number names nobody.
        let pid = reaped_child().id();

        let h = TestHeap::new();
        let handle_val = h
            .ctx()
            .external("process", ProcessHandle::new(pid, reaped_child()));

        let (bits, answer) = kill(&h, handle_val, "sigterm");
        assert_eq!(bits, SIG_OK, "a pid nobody holds is not an error");
        assert_eq!(
            answer,
            Value::keyword("missing"),
            "the answer must report the pid, not infer a child from it"
        );
    });
}

/// A kill on a handle whose child is still there sends the signal and says so.
///
/// The other half of the answer: without this, "send nothing" would pass by
/// never sending anything.
#[test]
fn a_kill_on_a_live_child_signals_it() {
    crate::value::arena::with_test_region(|| {
        let child = long_lived_child();
        let pid = child.id();

        let h = TestHeap::new();
        let handle_val = h.ctx().external("process", ProcessHandle::new(pid, child));

        let (bits, answer) = kill(&h, handle_val, "sigkill");
        assert_eq!(bits, SIG_OK, "signalling a live child succeeds");
        assert_eq!(
            answer,
            Value::keyword("signaled"),
            "the answer must say the signal was sent"
        );

        // `SIGKILL` is not instant: the child is reapable once the kernel has
        // torn it down, so ask until it is rather than once.
        let record = handle_val
            .as_external::<ProcessHandle>()
            .expect("the handle is a process")
            .exit();
        loop {
            match record.reap(pid) {
                Reap::Exited(code) => {
                    assert_eq!(code, -libc::SIGKILL, "the child died from the signal sent");
                    break;
                }
                Reap::Running => std::thread::sleep(Duration::from_millis(10)),
                Reap::Failed(errno) => panic!("waitpid failed: errno {}", errno),
            }
        }
    });
}
