# Cancelling an operation

<!-- audited: 2026-11-12 -->

`io/cancel` asks an operation to stop and marks its id, and the completion that still arrives is retired rather than delivered.

`ev/timeout` cancels an operation on every call, the body's or the timer's,
whichever lost. So this path runs constantly rather than at the edges.

## The two halves

**Asking the operation to stop** differs by platform. The ring takes an
`IORING_OP_ASYNC_CANCEL` SQE, whose own CQE carries the high-bit tag and is
skipped, and the operation's CQE then arrives with `-ECANCELED`. A pool worker
is asked through its stop pipe, below.

**Marking the id** is the same on both. `PendingTable::mark_cancelled` records
the id, and the completion that arrives for it retires its entry instead of
building a result. `AsyncBackend::cancel`
([poll.rs](../../../src/io/aio/poll.rs)) does both halves.

Every caller of `io/cancel` in the scheduler drops its own record of the
submission first: `complete-fiber`, `handle-abort`, `handle-io-forward-cancel`
and `do-shutdown`. So the id is marked exactly when nobody is left to read it.

Backend teardown marks everything in flight the same way, through
`PendingTable::cancel_all` from `quiesce_pending`, because the heap its values
live on may already be gone.

A cancel for an id that is no longer in flight marks nothing. That operation
completed, and its result already reached the fiber that asked. A mark left
behind would meet a later submission. A write still waiting in line is not in
flight either, and a cancel takes it out of its line ([writes](write.md)).

## What a cancel promises, on either platform

- **The submission is accounted for.** The entry stays in the table until the
  completion arrives. The completion still finds it, and on the pool it still
  decrements the hub's `in_flight`. Removing the entry at the cancel would
  strand the worker's thread, and decrementing there too would count the
  operation twice.
- **No completion is delivered.** `PendingTable::take` reports the submission
  cancelled, and its entry is retired rather than cooked. Retiring releases the
  pooled buffer, closes a descriptor the completion would have wrapped in a
  port, and frees a process wait's `siginfo_t`. Cooking it anyway would read
  the port, the process handle or the fiber's buffer after the finished fiber's
  release freed the regions they live in.
- **The descriptor outlives the operation**
  ([descriptors and workers](descriptor.md)).

What the entry lets go of at the mark, and what it keeps until the completion,
is [an operation in flight](inflight.md). A read that borrowed its port's
remainder gives it back at the cancel itself
([where a stream operation's bytes live](bytes.md)).

Pinned by `a_cancelled_operation_delivers_no_completion_on_either_backend`
([park.rs](../../../src/io/aio/tests/park.rs)), which holds both platforms to
the one answer.

## The stop pipe

An operation that can wait indefinitely carries a **stop pipe**, and that is
what lets it stop at once rather than when its peer happens to act.
`CompletionHub::bounds` opens one per submission and hands it to the worker
inside the operation's `Bounds`. The worker owns the read end for the
operation's lifetime and polls it beside its own descriptor through `OpBound`,
and `CompletionHub::stop` writes one byte into the write end. The operation
then completes with `-ECANCELED`, which gives the worker thread back like any
other completion.

Polling a pipe keeps the descriptor intact. Shutting the socket down would
reach the worker too, and would break a port the caller still holds. A signal
would land on whichever thread the kernel chose.

Two conditions decide which operations carry one:

- **It can wait for something that may never happen.** The reads, the write,
  `Sleep`, `Accept`, `RecvFrom`, both connects, `PollFd`, `WatchRead`, the
  signal reads, `ProcessWait` and `Open` all wait on something outside this
  process: a peer, an event, a child, a fifo's other end. The write's peer is
  the one that reads. A payload larger than the send buffer only leaves as the
  peer takes what is already in it, so a peer that stops reading parks the
  write in `bound.wait(POLLOUT)` ([writes](write.md)). `Flush`, `SendTo` and
  `Shutdown` transfer what the process already handed over, and a `File` sync
  opens its path with `O_NONBLOCK`. Those four take no pipe
  (`Bounds::prompt`).
- **Stopping it must not park it instead.** `OpBound` takes the descriptor
  non-blocking for the operation's lifetime, so the syscall answers `EAGAIN`
  and hands the wait back to the poll where the stop is visible. A worker that
  calls the blocking syscall first is out of reach: closing a listener does not
  wake a thread already inside `accept(2)`.

Closing the descriptor is not a second way to end the operation. Whether a
close wakes a thread parked in `poll(2)` on that descriptor is the platform's
choice. macOS and the BSDs wake it. Linux does not, because `poll` holds a
reference to the file it waits on. So an operation that can park carries a pipe
on every platform, and a close ends it by writing to that pipe
([descriptors and workers](descriptor.md)).

Two operations meet the first condition and cannot meet the second: an `Open`
of a fifo for writing, and a `ProcessWait` on a child that has not exited. The
kernel reports no readiness for what they wait on. Both ask with a
non-blocking form, `O_NONBLOCK` and `WNOHANG`, and `pace_retry` waits between
asks with the stop pipe visible throughout. `connect_bounded` does the same for
an AF_UNIX peer whose backlog is full.

`Resolve` and `Task` meet neither, and say so with `Bounds::uninterruptible`.
`getaddrinfo(3)` runs to the resolver's own end, and an opaque closure runs
until it returns. A cancel discards the result without giving the worker thread
back any sooner. Every use of that constructor names the call that behaves
this way.

A `File` read, write, append or copy meets the first condition on a fifo,
whose other end need never open, and cannot meet the second. It runs through
blocking `std::fs` calls, which a stop pipe cannot reach, so it says so with
`Bounds::uninterruptible` too.

When the process is out of descriptors, `bounds` returns a `Bounds` with no
stop pipe, and the operation runs uncancellable. It still bounds itself by the
caller's `:timeout`, which `OpBound` enforces with the same poll
([how an operation is bounded](bounds.md)).

## Pinned by

- [io-cancel-releases.lisp](../../../tests/lang/io-cancel-releases.lisp), for
  the descriptor a cancelled operation gives back.
- [io-cancel-workers.lisp](../../../tests/impl/io-cancel-workers.lisp), which
  counts the workers.
