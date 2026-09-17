//! audited: 2026-09-17
//! `subprocess/kill` against a recorded exit status, and the boundary every
//! subprocess primitive refuses through.

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
        let handle_val = h.ctx().external(SUBPROCESS, handle);

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
            .external(SUBPROCESS, ProcessHandle::new(pid, reaped_child()));

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
        let handle_val = h.ctx().external(SUBPROCESS, ProcessHandle::new(pid, child));

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

/// The error message a primitive answered with, or `None` for a success.
fn refusal(answer: (SignalBits, Value)) -> Option<String> {
    let (bits, err) = answer;
    if bits != SIG_ERROR {
        return None;
    }
    let fields = err.as_struct().expect("an error is a struct");
    sorted_struct_get(fields, &TableKey::keyword("message"))?.with_string(|s| s.to_string())
}

/// Every primitive that takes a subprocess refuses anything else, at the
/// boundary, with one message.
///
/// The trap: the struct built here is shaped exactly like the exec result this
/// type replaced — it carries a `:process` key. The extractor that read that key
/// without checking what sat under it let such a struct through, and each
/// primitive then failed on its own, several steps later, with a message of its
/// own. So a decoy with the key is what tells a boundary check from a use-site
/// one; a decoy without it would be refused either way.
///
/// The counter-factual is three different messages. Asserting they are equal is
/// what says one check answered for all three, rather than three checks that
/// happen to agree today.
#[test]
fn every_subprocess_primitive_refuses_a_non_subprocess_alike() {
    crate::value::arena::with_test_region(|| {
        let h = TestHeap::new();
        let decoy = {
            let ctx = h.ctx();
            let mut fields = std::collections::BTreeMap::new();
            fields.insert(TableKey::keyword("pid"), Value::int(1));
            fields.insert(TableKey::keyword("process"), Value::int(42));
            ctx.struct_from(fields)
        };

        let messages: Vec<String> = ["wait", "kill", "pid"]
            .iter()
            .map(|which| {
                let mut ctx = h.ctx();
                let answer = match *which {
                    "wait" => prim_subprocess_wait(&mut ctx, &[decoy]),
                    "kill" => prim_subprocess_kill(&mut ctx, &[decoy]),
                    _ => prim_subprocess_pid(&mut ctx, &[decoy]),
                };
                refusal(answer).unwrap_or_else(|| panic!("subprocess/{which} accepted a struct"))
            })
            .collect();

        for (which, message) in ["wait", "kill", "pid"].iter().zip(&messages) {
            assert!(
                message.starts_with(&format!("subprocess/{which}: ")),
                "the refusal names the primitive that made it: {message}"
            );
            assert!(
                message.contains("expected a subprocess"),
                "the refusal says what it wanted: {message}"
            );
            assert!(
                message.contains("struct"),
                "and what it got instead: {message}"
            );
        }

        let bodies: Vec<&str> = messages
            .iter()
            .map(|m| m.split_once(": ").expect("a named refusal").1)
            .collect();
        assert!(
            bodies.windows(2).all(|w| w[0] == w[1]),
            "one check answers for all three, so the bodies agree: {bodies:?}"
        );
    });
}
