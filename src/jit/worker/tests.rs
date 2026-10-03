// audited: 2026-10-01
// docs/impl/jit.md
//! A worker that its VM drops discards its queue instead of compiling it.

use super::*;
use crate::lir::testkit::LirFixture;
use crate::lir::{LirInstr, Reg, Terminator};
use crate::signals::Signal;
use crate::value::Arity;

/// fn(x) -> x: the cheapest function the compiler accepts, so the queue, not
/// any one compile, is what the test measures.
fn identity_lir() -> LirFunction {
    LirFixture::new(Arity::Exact(1))
        .signal(Signal::silent())
        .block(
            0,
            vec![LirInstr::LoadCapture {
                dst: Reg(0),
                index: 0,
            }],
            Terminator::Return(Reg(0)),
        )
        .build()
}

/// The counter-factual is a worker that drains its queue after the drop: every
/// task compiles, and the count equals the queue. Ten thousand tasks is far more
/// than the thread can finish while this thread queues them and drops.
#[test]
fn dropping_the_worker_discards_its_queue() {
    let queued = 10_000;
    let lir = identity_lir();
    let worker = JitWorker::new();
    // The thread holds the only sender of results, so counting them ends when
    // the thread exits.
    let results = worker.rx.clone();
    for key in 0..queued {
        assert!(
            worker.submit(prepare_task(&lir, key, None)),
            "the worker refused a task"
        );
    }
    drop(worker);

    let compiled = results.iter().count();
    assert!(
        compiled < queued,
        "the worker compiled all {queued} queued tasks after its VM dropped it"
    );
}
