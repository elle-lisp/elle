// audited: 2026-10-20
// docs/impl/jit.md
//! A worker that its VM drops discards its queue and stops the compile in
//! progress.

use super::*;
use crate::lir::testkit::LirFixture;
use crate::lir::{BinOp, InstrRef, Label, LirOwned, Reg, Terminator};
use crate::signals::Signal;
use crate::value::Arity;

/// fn(x) -> x: the cheapest function the compiler accepts, so the queue, not
/// any one compile, is what the test measures.
fn identity_lir() -> LirOwned {
    LirFixture::new(Arity::Exact(1))
        .signal(Signal::silent())
        .block(
            0,
            &[InstrRef::LoadCapture {
                dst: Reg(0),
                index: 0,
            }],
            Terminator::Return(Reg(0)),
        )
        .build()
}

/// fn(x): `blocks` blocks in a chain, each adding `x` to a running sum. The
/// operands are unproven, so each add is a tag-check diamond with a helper
/// call, and translating the chain takes long enough to drop a worker in.
fn long_lir(blocks: u32) -> LirOwned {
    let mut fixture = LirFixture::new(Arity::Exact(1)).signal(Signal::silent());
    fixture = fixture.block(
        0,
        &[InstrRef::LoadCapture {
            dst: Reg(0),
            index: 0,
        }],
        Terminator::Jump(Label(1)),
    );
    for block in 1..blocks {
        let terminator = if block + 1 == blocks {
            Terminator::Return(Reg(block))
        } else {
            Terminator::Jump(Label(block + 1))
        };
        fixture = fixture.block(
            block,
            &[InstrRef::binop(
                Reg(block),
                BinOp::Add,
                Reg(block - 1),
                Reg(0),
            )],
            terminator,
        );
    }
    fixture.build()
}

/// A task crosses to the worker thread, so it must be `Send` — and by its type,
/// since the frozen LIR it carries is plain data and its values are two words
/// each. A hand-written `unsafe impl` would hold this however the task grew,
/// which is why it is the claim the type should carry instead.
#[test]
fn a_jit_task_is_send_by_type() {
    fn assert_send<T: Send>() {}
    assert_send::<JitTask>();
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
            worker.submit(prepare_task(&lir.view(), key, None)),
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

/// The counter-factual is a worker that lets the compile in progress run to
/// its end: the thread delivers that one result after the drop.
#[test]
fn dropping_the_worker_stops_the_compile_in_progress() {
    let lir = long_lir(20_000);
    let worker = JitWorker::new();
    let results = worker.rx.clone();
    assert!(
        worker.submit(prepare_task(&lir.view(), 0, None)),
        "the worker refused a task"
    );
    // Trap: a drop before the thread takes the task discards it from the
    // queue, which passes this test with no compile ever started. Wait until
    // the queue is empty, then long enough for the thread to start compiling.
    while !worker.tx.is_empty() {
        std::thread::yield_now();
    }
    std::thread::sleep(std::time::Duration::from_millis(50));
    let dropped = std::time::Instant::now();
    drop(worker);

    // The thread holds the only sender of results, so the count ends when the
    // thread exits.
    let delivered = results.iter().count();
    assert_eq!(
        delivered,
        0,
        "the compile in progress delivered a result {:?} after its VM dropped the worker",
        dropped.elapsed()
    );
}
