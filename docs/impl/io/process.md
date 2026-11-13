# Waiting on a child

<!-- audited: 2026-11-11 -->

How a spawn and a wait run on each platform, and why a child's exit status lives in a record rather than in a completion.

[Subprocesses](../../subprocess.md) says what a program sees: the
`subprocess` value, its streams, and what a wait and a kill answer. The handle
and its record are `ProcessHandle` and `ExitRecord`
([process.rs](../../../src/io/request/process.rs)), each documented there.

## A spawn finishes inside the call

`SpawnRequest::spawn_to_subprocess` ([spawn.rs](../../../src/io/request/spawn.rs))
starts the child with `std::process::Command` and builds the handle and its
stdio ports at the completion's `Birthplace`. That is one region on the
requesting instance's heap, whose one reference the completion hands over when
it becomes a value ([an operation in flight](inflight.md)). So the ports and
the handle hold no reference across heaps, and the region they share is
nobody's to leak.

No CQE arrives for a spawn. `submit_spawn`
([externals.rs](../../../src/io/aio/externals.rs)) pushes the completion
straight onto the queue.

## A wait runs on either platform

`submit_process_wait` reads the handle's record before it submits anything.
When the record already holds a status, the wait answers at once and files no
entry. So a status is delivered to a waiter, or held until one asks, which also
covers a child that is waited on twice.

| Platform | How it waits | How it reaps |
|----------|--------------|--------------|
| io_uring | `IORING_OP_WAITID` with `WEXITED \| WNOWAIT`, which needs Linux 6.7 or later; an older kernel answers `-EINVAL` in the CQE | the completion reaps through `ExitRecord::reap` |
| thread pool | `child::process_wait` asks with `wait4(pid, .., WNOHANG, ..)`, and `pace_retry` waits between asks with the stop pipe visible | the ask itself, through `ExitRecord::reap` |

`pace_retry` starts at a millisecond and grows to fifty, so a child that exits
at once is reported at once, while a long-running one costs few wakeups. A
blocking wait would hold the worker for the child's whole life, where neither
`io/cancel` nor a deadline can reach it.

The ring's kernel fills a `siginfo_t` when the child exits. The entry
allocates it with `Box::into_raw` and owns it until the CQE arrives, and
completion processing or `PendingOp::retire` reclaims it with `Box::from_raw`.
The fast path allocates none, because it files no entry.

## A reap is never wasted

One operation cannot keep the promise that a cancelled operation delivers no
completion ([cancelling an operation](cancel.md)). A reap **consumes** what it
reports: the kernel hands a child's exit status over once, and then the child
is gone. A wait cancelled just after its reap has already spent the status,
and the promise would throw that status away. The next `subprocess/wait` on
that child would then find no child, and report `ECHILD` for a status this
process took.

So the status does not travel in the completion alone. `ExitRecord` is where
whoever reaps puts it, and every `ProcessHandle` holds one. The pending entry
and the pool operation each carry a clone, so a write reaches no heap value. A
worker that finishes after a teardown keeps the status without dereferencing a
handle whose region may already be gone.

The alternative was to narrow the window rather than close it: check the stop
pipe immediately before each `wait4` instead of only between them. The check
and the syscall cannot be one step, so a cancel that lands between them still
reaps, and the guarantee could not be stated.

Both platforms reap through `ExitRecord::reap`, **under the record's lock**,
which makes the ask and the answer one step. `reap` returns the status an
earlier reap left, so a second waiter never sees the gap between another's
`wait4` and its write. The ring asks with `WNOWAIT` because it has no
`rusage` to fill: the kernel reports the exit and leaves the child for the
completion to reap, so a cancelled ring wait reaped nothing. A `waitid` that
finds no child answers from the record instead.

## The record says whether a pid is still this child

The kernel returns a reaped pid to its pool and hands it out again. So
`subprocess/kill` reads the record before it calls `kill(2)`: a handle that
holds a status has no child of its own left, and the call sends nothing rather
than signal whatever holds that number now. A waiter and a killer read the
same fact from the record, that the child is gone.

## Pinned by

- `a_kill_on_a_reaped_child_sends_no_signal`
  ([tests.rs](../../../src/primitives/subprocess/tests.rs)), with a handle
  built over a pid that names another process, since a test cannot recycle a
  pid on demand.
- `a_stopped_wait_that_reaped_the_child_keeps_its_status` and
  `a_wait_on_an_already_reaped_child_answers_from_the_record`
  ([process.rs](../../../src/io/threadpool/tests/process.rs)), for the worker.
- In [process.rs](../../../src/io/aio/tests/process.rs):
  `a_cancelled_wait_that_reaped_the_child_answers_the_next_wait` runs the
  whole path on the pool,
  `a_cancelled_uring_wait_leaves_the_child_for_the_next_wait` holds the ring's
  cancelled wait to reaping nothing, `a_wait_on_a_held_status_files_no_operation`
  holds the fast path to issuing nothing, and
  `a_wait_that_finds_no_child_answers_from_the_record` builds the loser's
  `ECHILD` at the entry, which no test can make two waits collide to produce.
