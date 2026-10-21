// audited: 2026-10-21
//! The residue gates of a run that reaches the io backend, the scheduler, or the store of compiled files.
//!
//! docs/impl/region/rules.md
//! docs/impl/io-inflight.md

use super::census::{residue_after_teardown, HandOff};
use super::*;
use elle::compiler::stdlib_cache::StdlibCache;

/// A run that spawns a child leaves nothing either. The `subprocess` value is
/// built when the spawn finishes rather than by the call that asked for it, so
/// the region it lives in is one the completion owns and hands over
/// (docs/impl/io-inflight.md).
///
/// The counter-factual is `teardown_leaves_no_residue`
/// (tests/region_process_teardown/census.rs), which cannot see this at all.
/// Neither of its programs reaches the io backend, so no completion in that run
/// ever builds a value. A completion that keeps what it
/// built leaves one region per child, linear in the number of children, and
/// every other gate reads clean while it does.
///
/// The child is `/bin/sh -c :`, an absolute path so no `PATH` decides whether
/// this test measures a spawn or an `exec-error`. Both answers are built the
/// same way, so the error would read as a pass.
#[test]
fn a_run_that_spawns_a_child_leaves_no_residue() {
    let src = "(let [p (subprocess/exec \"/bin/sh\" [\"-c\" \":\"])] \
               (subprocess/wait p))";
    assert_eq!(
        residue_after_teardown(
            Runtime::with_stdlib_cache(StdlibCache::Off),
            src,
            HandOff::Root
        ),
        0,
        "{src}: regions survived teardown",
    );
}

/// A run that bounds work with a deadline leaves nothing either. `ev/timeout`
/// spawns the body and a timer, and aborts whichever lost — so a program that
/// calls it once ends with one fiber aborted through the scheduler and one I/O
/// operation cancelled.
///
/// Three holders keep an aborted fiber, and all three have to let go for this
/// to read zero. The first is the loop's completion record and its mark, both
/// keyed by the fiber (docs/scheduler.md). The second is the runnable queue,
/// which a completed fiber leaves on the same rule. The third is the cancelled
/// operation's own entry, which retains the fiber it would have answered
/// (docs/impl/io-inflight.md).
///
/// The counter-factual is `teardown_leaves_no_residue`
/// (tests/region_process_teardown/census.rs), which reaches none of this.
/// Neither of its programs spawns a fiber, so no fiber is ever completed,
/// aborted or cancelled for. Each holder is worth the fiber, its closure and the
/// payload the abort delivered, and every other gate reads clean while they
/// hold.
///
/// The body wins on purpose. A timer of 30 seconds cannot fire inside a test,
/// so the abort always lands on the timer and the run never waits for one. A
/// body that wins at once is also what leaves the loop no chance to reap the
/// cancellation it just issued.
#[test]
fn a_run_that_times_out_leaves_no_residue() {
    let src = "(ev/timeout 30 (fn [] 1))";
    assert_eq!(
        residue_after_teardown(
            Runtime::with_stdlib_cache(StdlibCache::Off),
            src,
            HandOff::Root
        ),
        0,
        "{src}: regions survived teardown",
    );
}

/// A run that leaves a fiber sleeping leaves nothing either. The program ends
/// while its spawned fiber is parked in `ev/sleep`, so `do-shutdown` cancels
/// the operation and aborts the fiber — and everything the loop remembered
/// must still come apart.
///
/// The counter-factual is `a_run_that_times_out_leaves_no_residue` beside it,
/// whose abort travels through `handle-abort` on the program's own request.
/// This one is the shutdown's abort, and its residue turns on the binding layer
/// rather than the scheduler: `:pump`'s unjoined-error scan conditionally
/// assigns each snapshot entry's fiber to a reassigned local
/// (docs/impl/region/bindings.md). A stored value's release that rode the
/// binding chain out of the loop would strand the snapshot, the fiber inside it,
/// and everything the fiber's closure reaches.
///
/// The timer sleeps 30 seconds so it cannot fire inside the test; two
/// sleepers pin the per-fiber slope, not only the constant.
#[test]
fn a_run_that_leaves_a_fiber_sleeping_leaves_no_residue() {
    for src in [
        "(ev/run (fn [] (ev/spawn (fn [] (ev/sleep 30))) 1))",
        "(ev/run (fn [] (ev/spawn (fn [] (ev/sleep 30))) \
                        (ev/spawn (fn [] (ev/sleep 30))) 1))",
    ] {
        assert_eq!(
            residue_after_teardown(
                Runtime::with_stdlib_cache(StdlibCache::Off),
                src,
                HandOff::Root
            ),
            0,
            "{src}: regions survived teardown",
        );
    }
}

/// The same claim for the other shape a completion builds: bytes whose length
/// nothing could reserve ahead of the read.
///
/// `port/read` and `port/read-line` answer from a buffer the call pre-allocated
/// and reach none of this, so a read alone is no evidence. `read-all` has no
/// count to reserve against, so its answer is born when the stream ends — which
/// is what makes it the discriminator for the hand-over the spawn case pins.
#[test]
fn a_run_that_reads_a_whole_file_leaves_no_residue() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("residue-read-all");
    std::fs::write(&path, b"census").expect("write");
    let src = format!(
        "(let [p (port/open \"{}\" :read) s (port/read-all p)] (port/close p) (length s))",
        path.display()
    );
    assert_eq!(
        residue_after_teardown(
            Runtime::with_stdlib_cache(StdlibCache::Off),
            &src,
            HandOff::Root
        ),
        0,
        "{src}: regions survived teardown",
    );
}

/// A run that loads one file twice leaves nothing, and no reference the region
/// graph cannot explain. The instance keeps the file's compiled code from Rust,
/// and the second load runs it, so teardown has to give that hold back.
///
/// The counter-factual is `teardown_leaves_no_residue`
/// (tests/region_process_teardown/census.rs), whose programs load no file. A
/// kept unit that teardown never released pins its
/// code region from outside the graph, and every other gate reads clean.
#[test]
fn a_run_that_loads_a_file_twice_leaves_no_residue() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("residue-load.lisp");
    std::fs::write(&path, "(def items @[1 2])\n(fn [] items)\n").expect("write");
    let src = format!(
        "(let [a ((import-file \"{p}\")) b ((import-file \"{p}\"))] (+ (length a) (length b)))",
        p = path.display()
    );
    assert_eq!(
        residue_after_teardown(
            Runtime::with_stdlib_cache(StdlibCache::Off),
            &src,
            HandOff::Root
        ),
        0,
        "{src}: regions survived teardown",
    );
    let pinned = pinned_after_teardown(Runtime::with_stdlib_cache(StdlibCache::Off), &src);
    assert!(
        pinned.is_empty(),
        "{src}: regions pinned from outside the region graph: {pinned:#?}"
    );
}

/// One call whose completions build more than one answer: `subprocess/system`
/// spawns the child, reads both pipes to the end, and waits
/// (docs/impl/io-inflight.md).
///
/// It reaches what the two cases above do not. The `read-all` there reads a file
/// its own call opened; this one reads a pipe, through a port a completion built
/// and handed over. And neither of those can price a call that builds twice:
/// with the handover removed, the spawn above leaves 1 region behind and the
/// file read-all 3, where this leaves 5.
///
/// The child prints, so the `read-all` on its stdout answers with bytes rather
/// than the `nil` an empty pipe gives — the shape a caller reads. It is named by
/// an absolute path for the reason the spawn case gives: an `exec-error` is
/// built the same way, so it would read as a pass.
#[test]
fn a_run_that_captures_what_a_child_wrote_leaves_no_residue() {
    let src = "(get (subprocess/system \"/bin/sh\" [\"-c\" \"echo census\"]) :exit)";
    assert_eq!(
        residue_after_teardown(
            Runtime::with_stdlib_cache(StdlibCache::Off),
            src,
            HandOff::Root
        ),
        0,
        "{src}: regions survived teardown",
    );
}

/// A run that gives the scheduler more than one thunk leaves nothing either.
/// `ev/run` records a completion per entry fiber and, once the work is done,
/// walks those records looking for an error nobody joined. The walk binds a
/// name to each element and assigns it to a mutable it reads afterward.
///
/// The counter-factual is the single-thunk call, which every other gate
/// already covers by proxy. Were the walk's element read as a second holder of
/// the record collection, `(ev/run (fn [] 1))` would still read zero, because
/// one entry leaves nothing for the loop to displace. Two entries strand the whole
/// record collection, and the fibers it points at with it — 103 regions where one
/// thunk leaves 0, plus 2 more per extra thunk.
///
/// Three thunks rather than two, so the residue is priced on the slope as well
/// as on the zero: a per-entry strand grows with the entry count, and a fixed
/// one-off would not.
#[test]
fn a_run_with_several_thunks_leaves_no_residue() {
    for src in [
        "(ev/run (fn [] 1) (fn [] 2))",
        "(ev/run (fn [] 1) (fn [] 2) (fn [] 3))",
    ] {
        assert_eq!(
            residue_after_teardown(
                Runtime::with_stdlib_cache(StdlibCache::Off),
                src,
                HandOff::Root
            ),
            0,
            "{src}: regions survived teardown",
        );
    }
}
