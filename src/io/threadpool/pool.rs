//! The crew of worker threads a [`CompletionHub`] runs its operations on.
//!
//! A worker outlives the operation it was started for: it parks on the job
//! queue and takes the next submission, so a program that keeps asking for I/O
//! pays for a thread once rather than per operation. See src/io/AGENTS.md
//! § "How a worker is reused" for what that costs and what it must not cost.

use super::*;
use crossbeam_channel::Sender;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

/// How long a parked worker waits for a job when the caller named no keepalive
/// of its own — `(io/backend :async nil)`, and every backend built from Rust.
///
/// The number trades two costs, and neither is a cliff: a worker that retires
/// too eagerly makes the next operation pay for a thread, and one that lingers
/// holds a stack and costs a wakeup per period. Ten seconds is longer than the
/// gap between operations of a program that is doing I/O at all, and short
/// enough that one which has stopped gives its threads back while it is still
/// running. A program with a reason to prefer another number says so through
/// `*io-keepalive*` (docs/parameters.md) rather than living with this one.
pub(in crate::io) const DEFAULT_KEEPALIVE: Duration = Duration::from_secs(10);

/// One operation, and everything the worker that runs it needs to report it.
///
/// The channel and the bridge descriptor travel with the job rather than with
/// the worker: a worker serves whatever is queued, and each job answers to the
/// hub that submitted it.
pub(super) struct Job {
    pub(super) id: u64,
    pub(super) kind: OpKind,
    pub(super) op: PoolOp,
    pub(super) bounds: Bounds,
    pub(super) sender: Sender<RawCompletion>,
    pub(super) eventfd: Option<RawFd>,
}

impl Job {
    /// Run the operation to its result and package that for the hub channel.
    fn run(self) -> Delivery {
        let Job {
            id,
            kind,
            op,
            bounds,
            sender,
            eventfd,
        } = self;
        let (result_code, data) = submitop::run(op, bounds);
        Delivery {
            sender,
            eventfd,
            completion: RawCompletion::Pool(PoolCompletion {
                id,
                kind,
                result_code,
                data,
            }),
        }
    }
}

/// A finished operation's result, and where it goes back to.
struct Delivery {
    sender: Sender<RawCompletion>,
    eventfd: Option<RawFd>,
    completion: RawCompletion,
}

impl Delivery {
    fn publish(self) {
        publish_completion(&self.sender, self.eventfd, self.completion);
    }
}

/// The workers a hub runs operations on, and the handoffs that reach the parked
/// ones.
pub(super) struct WorkerPool {
    /// What the workers share.
    crew: Arc<Crew>,
    /// How many threads this pool has started. Names them, and the number is
    /// the worker's identity when it withdraws its own handoff.
    started: u64,
}

/// The half of the pool a worker holds: the parked workers' handoffs and how
/// long one waits before retiring.
struct Crew {
    parked: Mutex<Parked>,
    /// How long a worker waits for another job before it retires — what the
    /// program bound `*io-keepalive*` to, or [`DEFAULT_KEEPALIVE`]. Zero turns
    /// reuse off, and `next_job` is where that is exact.
    keepalive: Duration,
}

/// The parked workers, and whether the pool they serve is still there.
struct Parked {
    /// One handoff per parked worker, **most recently parked last**.
    ///
    /// A submission takes from the end, so the next job goes to the worker that
    /// stopped working most recently — the warm one — and the workers the
    /// traffic no longer reaches sit at the bottom and age out of the
    /// keepalive. See src/io/AGENTS.md § "How a worker is reused" for what this
    /// order is and is not known to buy.
    workers: Vec<Handoff>,
    /// True once the pool is gone. A worker still running a job when that
    /// happens retires when it finishes rather than parking for a keepalive
    /// nobody will interrupt.
    closed: bool,
}

/// One parked worker: the slot a submission leaves its job in, and the identity
/// the worker withdraws that slot by.
struct Handoff {
    worker: u64,
    slot: Arc<Slot>,
}

/// Where one worker waits, and what it waits for.
///
/// A condition variable rather than a channel, because this wait happens once
/// per operation and a channel receiver **spins** before it sleeps. On a
/// machine with more cores than threads that spin is free and often saves the
/// sleep; on one with fewer, it burns the cores the rest of the program is
/// waiting for — twice the user CPU for the heaviest corpus files, measured on
/// a three-core runner. `wait_timeout` sleeps immediately, so an operation that
/// hands work over pays a wake and nothing else.
struct Slot {
    state: Mutex<SlotState>,
    filled: Condvar,
}

struct SlotState {
    /// The job a submission left here, taken by the worker that owns the slot.
    job: Option<Job>,
    /// True once the pool is gone, so a parked worker stops waiting for a job
    /// that can no longer be handed to it.
    closed: bool,
}

impl Slot {
    fn new() -> Arc<Slot> {
        Arc::new(Slot {
            state: Mutex::new(SlotState {
                job: None,
                closed: false,
            }),
            filled: Condvar::new(),
        })
    }

    /// Leave `job` for the worker that owns this slot and wake it. The slot is
    /// empty by construction: a submission gets here only by taking this
    /// worker's handoff, and the worker posts one handoff at a time.
    fn fill(&self, job: Job) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.job = Some(job);
        drop(state);
        self.filled.notify_one();
    }

    /// Tell the worker that owns this slot that nothing more is coming.
    fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.closed = true;
        drop(state);
        self.filled.notify_one();
    }

    /// Wait up to `limit` for a job. `None` means the wait ended without one —
    /// the keepalive elapsed, or the pool went away.
    fn wait(&self, limit: Duration) -> Option<Job> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let deadline = Instant::now() + limit;
        loop {
            if let Some(job) = state.job.take() {
                return Some(job);
            }
            if state.closed {
                return None;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            state = self
                .filled
                .wait_timeout(state, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    /// Wait without a deadline for the job a submission has already committed
    /// to leaving here.
    fn wait_committed(&self) -> Option<Job> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(job) = state.job.take() {
                return Some(job);
            }
            if state.closed {
                return None;
            }
            state = self.filled.wait(state).unwrap_or_else(|e| e.into_inner());
        }
    }
}

impl WorkerPool {
    pub(super) fn new(keepalive: Duration) -> Self {
        WorkerPool {
            crew: Arc::new(Crew {
                parked: Mutex::new(Parked {
                    workers: Vec::new(),
                    closed: false,
                }),
                keepalive,
            }),
            started: 0,
        }
    }

    /// How long this pool's workers wait for another job before retiring.
    #[cfg(test)]
    pub(super) fn keepalive(&self) -> Duration {
        self.crew.keepalive
    }

    /// Run `job`: hand it to a parked worker, or start one for it.
    ///
    /// The error is the OS refusing a thread. `Builder::spawn` rather than
    /// `thread::spawn` is what makes that a report: `thread::spawn` panics,
    /// while a refusal here becomes the error `io/submit` returns and the
    /// calling fiber can handle.
    pub(super) fn run(&mut self, job: Job) -> Result<(), String> {
        // A claimed worker waits on its slot until this job arrives in it: it
        // can leave only by withdrawing its own handoff under the lock the
        // claim just held, so the job reaches the worker the claim took.
        match self.crew.claim_parked() {
            Some(slot) => {
                slot.fill(job);
                Ok(())
            }
            None => self.start_worker(job),
        }
    }

    fn start_worker(&mut self, job: Job) -> Result<(), String> {
        let crew = Arc::clone(&self.crew);
        self.started += 1;
        let me = self.started;
        std::thread::Builder::new()
            .name(format!("elle-io-{}", me))
            .spawn(move || crew.work(me, job))
            .map(|_detached| ())
            .map_err(|e| format!("async I/O: cannot start a worker thread: {}", e))
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        // Closing every parked worker's slot ends its wait now rather than at
        // its keepalive, and the flag catches the workers still running a job:
        // both retire without a shutdown protocol and without anything to join.
        let mut parked = self.crew.parked.lock().unwrap_or_else(|e| e.into_inner());
        parked.closed = true;
        for handoff in parked.workers.drain(..) {
            handoff.slot.close();
        }
    }
}

impl Crew {
    /// Run `first`, then every job handed over afterwards, until this worker
    /// retires or the pool goes away. `me` is what its handoff is filed under.
    fn work(&self, me: u64, first: Job) {
        // Block every asynchronous signal on this worker so the kernel never
        // selects it as the delivery target for a watched POSIX signal. The
        // fault set stays deliverable. This is the thread's mask rather than
        // one job's, because the thread outlives the job.
        // See src/io/sigfd.rs and docs/posix-signals.md.
        crate::io::sigfd::mask_all_signals_on_this_thread();
        // One slot for this worker's whole life: it waits in the same place
        // after every operation, so parking allocates nothing.
        let slot = Slot::new();
        let mut job = first;
        loop {
            let delivery = job.run();
            match self.next_job(me, &slot, delivery) {
                Some(next) => job = next,
                None => return,
            }
        }
    }

    /// Post this worker's handoff, publish `delivery`, and wait for the next
    /// job.
    ///
    /// The handoff is posted before the completion is published because the
    /// completion is how a caller learns the operation ended, and the next
    /// submission follows it immediately: a worker that posted afterwards would
    /// be missed by the very submission it is there to take, which starts a
    /// thread instead. Nothing between the two can park — a send on an
    /// unbounded channel, and on the uring bridge an eventfd write.
    ///
    /// `None` retires this worker.
    fn next_job(&self, me: u64, slot: &Arc<Slot>, delivery: Delivery) -> Option<Job> {
        {
            let mut parked = self.parked.lock().unwrap_or_else(|e| e.into_inner());
            // Reuse turned off, or the pool gone: retire without ever posting a
            // handoff. Posting one and withdrawing it would leave an instant in
            // which a submission could still hand this worker a job, and a
            // program that asked for no reuse would see reuse anyway.
            if self.keepalive.is_zero() || parked.closed {
                drop(parked);
                delivery.publish();
                return None;
            }
            parked.workers.push(Handoff {
                worker: me,
                slot: Arc::clone(slot),
            });
        }
        delivery.publish();
        match slot.wait(self.keepalive) {
            // A submission took this worker's handoff and filled its slot.
            Some(job) => Some(job),
            // The keepalive elapsed, or the pool closed the slot.
            None => self.retire_or_wait(me, slot),
        }
    }

    /// Retire this worker if its handoff is still posted, or wait on for the
    /// job that took it.
    ///
    /// Withdrawing under the lock is what makes the two outcomes exclusive: a
    /// submission that claimed this worker took the handoff out of the list
    /// already, so finding nothing to withdraw means a job is on its way and
    /// leaving would strand it.
    fn retire_or_wait(&self, me: u64, slot: &Slot) -> Option<Job> {
        {
            let mut parked = self.parked.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(at) = parked.workers.iter().position(|h| h.worker == me) {
                parked.workers.remove(at);
                return None;
            }
        }
        slot.wait_committed()
    }

    /// Take the most recently parked worker's slot, if any worker is parked.
    fn claim_parked(&self) -> Option<Arc<Slot>> {
        let mut parked = self.parked.lock().unwrap_or_else(|e| e.into_inner());
        parked.workers.pop().map(|handoff| handoff.slot)
    }
}
