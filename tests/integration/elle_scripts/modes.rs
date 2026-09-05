// audited: 2026-09-05
// Scripts pinned to a backend toggle or an I/O backend rather than to the guardfree oracle.
//
// docs/analysis/testing.md

use super::*;

// Self-recursion correctness across control-flow boundaries, armed under the UAF
// oracle. A self-recursive local function must keep recursing as itself — same
// body, same captured environment — across a yield/resume, a tail-call frame
// replacement, or a value handoff. The corpus files assert the *values* (a stale
// self-reference returns a wrong-but-well-typed result the harness's vm/jit
// policies catch); these subprocess runs add the complementary guarantee that
// carrying the executing closure across each boundary reads no freed page — a
// botched self-identity that freed the live closure/env would fault here under
// guardfree rather than read recycled memory. `--jit=adaptive` exercises the
// hot-compiled path while the recursion is still in flight.
#[test]
fn recur_after_yield_guardfree() {
    run_elle_script_with_args(
        "recur-after-yield",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

#[test]
fn recur_after_tail_call_guardfree() {
    run_elle_script_with_args(
        "recur-after-tail-call",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

#[test]
fn recur_as_value_guardfree() {
    run_elle_script_with_args(
        "recur-as-value",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

#[test]
fn recur_entry_guardfree() {
    run_elle_script_with_args(
        "recur-entry",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// In-lambda MUTUAL recursion under the UAF oracle: the closure-cycle merge puts
// the ev/od pair and their forward cells in ONE arena, released either by the
// letrec binding-scope drop (non-tail body) or by the tail-call deferred release at the
// recursion's normal completion (tail body). A mis-accounted release — the arena
// freed while a rotation is still in flight, or freed twice across the two
// channels — reads a freed page here and faults deterministically.
#[test]
fn recur_mutual_guardfree() {
    run_elle_script_with_args(
        "recur-mutual",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// The adaptive-JIT build of the same entry-boundary coverage: the adaptive
// tier compiles a hot caller while its self-recursive callee is still
// interpreted — the compile-window shape the stdlib-HOF probe in the file
// exercises. The harness runs the file on the default (VM) tier; this
// subprocess covers the JIT half.
#[test]
fn recur_entry_jit() {
    run_elle_script_with_args("recur-entry", &["--jit=adaptive", "--mlir=off"]);
}

// Same script the harness already runs (`posix.lisp`), but forced onto the
// threadpool I/O backend on Linux via `--no-uring` — a process-global choice the
// harness cannot make per file. The threadpool path uses the same
// `SignalReceiver` / `kq_sig_read_blocking` / `sigfd_read_blocking` machinery as
// macOS, so this gates the threadpool signal flow (the signalfd EAGAIN-poll path
// on Linux and the EVFILT_SIGNAL worker-unblock + no-op sigaction path on
// macOS). Without it we'd only exercise the io_uring path on the Linux runner.
#[test]
fn posix_threadpool() {
    run_elle_script_with_args("posix", &["--no-uring"]);
}

// The full-write invariant on the OTHER backend. `port-shortwrite.lisp` proves
// that `port/write` transfers every byte of a payload far larger than one
// write(2) can move; the harness runs it on io_uring (the Linux default), where
// `drain_cqes` resubmits the unwritten tail. `--no-uring` is a process-global
// choice, so this pin is the only way to cover the thread-pool worker's own
// write loop (`PoolOp::Write`) on the Linux runner — the path macOS always
// takes. See src/io/AGENTS.md § Full-Write Invariant.
#[test]
fn port_shortwrite_threadpool() {
    run_elle_script_with_args("port-shortwrite", &["--no-uring"]);
}

// `:timeout` on a write that outgrows one syscall, on the OTHER backend. The
// two backends bound a blocked operation by different means — io_uring links a
// timeout SQE, the thread-pool worker relies on the fd's own send timeout — so
// each needs its own coverage of the re-armed deadline. Measured before the
// fix, both ignored `:timeout` on the resubmitted tail identically: the call
// blocked until the peer closed the socket and then reported ECONNRESET.
// See src/io/AGENTS.md § Full-Write Invariant.
#[test]
fn port_write_timeout_threadpool() {
    run_elle_script_with_args("port-write-timeout", &["--no-uring"]);
}

// `:timeout` on the looping reads, on the OTHER backend. io_uring re-arms a
// linked timeout on each resubmission; the thread-pool worker takes the fd
// non-blocking and waits in `poll(2)`. The pool half needs its own coverage
// twice over: it is the sole mechanism on macOS, and it was the weaker of the
// two before — measured on this file, io_uring already bounded a single
// `port/read` while the pool backend bounded no read at all.
// See src/io/AGENTS.md § Operation timeouts.
#[test]
fn port_read_timeout_threadpool() {
    run_elle_script_with_args("port-read-timeout", &["--no-uring"]);
}

// Grapheme-counted `read-exact` framing, on the OTHER backend. The two backends
// assemble a text read's answer from different places — io_uring from the
// fiber's buffer, the pool worker from the bytes it hands back — so each needs
// its own coverage of a cluster too wide for that buffer. The pool half is the
// sole mechanism on macOS. See docs/io.md § "A read that overshoots keeps the
// rest for the same port".
#[test]
fn port_text_framing_threadpool() {
    run_elle_script_with_args("port-text-framing", &["--no-uring"]);
}

// A line longer than the buffer `read-line` reserves, on the OTHER backend. The
// two backends outgrow that buffer in different places — the pool worker reads
// to the newline and hands back every byte at once, io_uring fills the buffer
// and resubmits — so each needs its own coverage. The pool half is the sole
// mechanism on macOS. See docs/io.md § "A read that overshoots keeps the rest
// for the same port".
#[test]
fn port_longline_threadpool() {
    run_elle_script_with_args("port-longline", &["--no-uring"]);
}

// Two timed operations on one descriptor, on the OTHER backend. The bound the
// pool worker uses is descriptor state the operations share, so the file only
// measures anything where that mechanism runs: io_uring gives each operation
// its own linked timeout and shares nothing. See the fixture header and
// src/io/AGENTS.md § Operation timeouts.
#[test]
fn port_timeout_shared_fd_threadpool() {
    run_elle_script_with_args("port-timeout-shared-fd", &["--no-uring"]);
}

// `:timeout` on the calls that wait for a peer, on the OTHER backend. io_uring
// links a timeout SQE to the accept and the receive; the pool worker has to
// bound them itself, waiting in `poll(2)` for the listener or the socket to be
// readable rather than parking in `accept(2)`/`recvfrom(2)` where no deadline
// can reach it. That mechanism is the only one on macOS, and it is the half
// this file measures — on io_uring the same script passes either way.
#[test]
fn net_wait_timeout_threadpool() {
    run_elle_script_with_args("net-wait-timeout", &["--no-uring"]);
}

// What a cancelled operation gives back, on the OTHER backend. `:workers`
// counts thread-pool operations submitted and not yet reaped, and io_uring runs
// most of these in the kernel — so it is zero there whatever the pool does. The
// worker half of the promise is only measurable here.
// See src/io/AGENTS.md § "I/O Cancellation".
#[test]
fn io_cancel_releases_threadpool() {
    run_elle_script_with_args("io-cancel-releases", &["--no-uring"]);
}

// An operation whose asking fiber is gone must end itself, on the OTHER
// backend. The pool ends one through its stop pipe and io_uring through
// `IORING_OP_ASYNC_CANCEL`, so neither half says anything about the other; the
// pool's is what macOS always runs. `:workers` measures the second claim the
// file makes — the thread comes back — and is zero on io_uring whatever the
// pool does. See src/io/AGENTS.md § "Ending an operation whose operands are
// gone".
#[test]
fn io_stale_operation_ends_threadpool() {
    run_elle_script_with_args("io-stale-operation-ends", &["--no-uring"]);
}

// Deep fiber/resume nesting must not consume the host call stack. The
// bytecode-VM path routes nested resumes through the SIG_SWITCH trampoline
// in `do_fiber_resume` (src/vm/fiber.rs), so 20000-deep nesting completes;
// pinned under the process-global `--jit=off` so the VM path is what runs.
// See the fixture header.
#[test]
fn fiber_deep_nesting_vm() {
    run_elle_file_with_args(
        "tests/integration/fixtures/fiber-depth.lisp",
        &["--jit=off"],
    );
}

// The same file under `--jit=eager`. The fixture's `-jit` driver shapes are
// JIT-admissible (their `fiber/new` lives in a helper, so the recursive
// resume caller itself compiles), so this pin drives a compiled
// `fiber/resume` caller 20000 deep — the depth a per-level Rust frame
// residue would turn into a stack-overflow abort. See the fixture header.
#[test]
fn fiber_deep_nesting_jit() {
    run_elle_file_with_args(
        "tests/integration/fixtures/fiber-depth.lisp",
        &["--jit=eager"],
    );
}

// A parked activation's region-map snapshot named a region it no longer owned,
// so on resume the debug-only uncounted-borrow guard (src/vm/core/resume.rs →
// `first_stale_borrow`, docs/impl/region/generations.md § "Uncounted-borrow
// check") aborted: "stale suspended-frame region borrow on resume".
//
// Root cause: the activation region map records `static slot → physical region`
// for every ALLOC-slot allocation and is cleared only by the slot-based
// `DecrefRegion`. A region freed any OTHER way — a value-based `DecrefValueRegion`/
// `DecrefCellRegion` (capture cells), a cross-region cascade, a subtree drop —
// leaves its entry behind, and the physical id it names is recycled to an
// unrelated region. `record_region_borrows` stamped each parked entry with the
// id's CURRENT generation, so such a leftover was snapshotted as a live borrow of
// an incarnation the activation never owned; when that unrelated incarnation was
// later freed, the resume check tripped. signals.lisp's cumulative squelch/
// silence/yield churn recycles ids fast enough to hit it (state-sensitive — it
// does not minimize to a small standalone form, hence the coupling to the file).
//
// Fixed by carrying the establish-generation in the map (`MappedRegion`): the
// snapshot records the generation the slot was valid at and skips entries whose
// region has since moved on (dead leftovers), while a genuine borrow freed *while
// parked* still trips the check. The abort was DEBUG-ONLY (release compiles the
// guard out and the leftover's dead `DecrefRegion` never reads it, so signals.lisp
// passed in CI's release corpus); this runs the file under the debug cargo-test
// profile where the guard is live.
#[test]
fn signals_no_stale_suspended_frame_region_borrow() {
    run_elle_script_with_args("signals", &["--jit=off"]);
}
