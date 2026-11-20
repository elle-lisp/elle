# Cancelling an operation

<!-- audited: 2026-11-20 -->

`io/cancel` asks an operation to stop and marks its id, and the completion that still arrives is retired rather than delivered.

`ev/timeout` cancels an operation on every call, the body's or the timer's,
whichever lost. So this path runs constantly rather than at the edges.

## The two halves

**Asking the operation to stop** differs by platform. The ring takes an
`IORING_OP_ASYNC_CANCEL` SQE, whose own CQE carries the high-bit tag and is
skipped, and the operation's CQE then arrives with `-ECANCELED`. An open that
waits in an io-wq thread answers `-ERESTARTSYS` instead. An operation a pool
worker runs is asked through its stop pipe, below, on either platform. A `File`
walk runs on a worker even where the ring runs the rest
([the two platforms](platforms.md)).

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
  pooled buffer, and frees a process wait's `siginfo_t` and the `statx` a stat
  on the ring fills. It closes a descriptor the completion would have wrapped
  in a port, or that a sync opened. It removes the directory a
  `file/mktempdir` made. Nobody else learns that directory's name, so it would
  stay in the temp root. Cooking the completion anyway would read the port,
  the process handle or the fiber's buffer after the finished fiber's release
  freed the regions they live in.
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

On the pool, two operations meet the first condition and cannot meet the
second: an `Open` of a fifo for writing, and a `ProcessWait` on a child that
has not exited. The kernel reports no readiness for what they wait on. Both ask
with a non-blocking form, `O_NONBLOCK` and `WNOHANG`, and `pace_retry` waits
between asks with the stop pipe visible throughout. `connect_bounded` does the
same for an AF_UNIX peer whose backlog is full. The ring needs no such retry: a
write-only open waits in an io-wq thread, and `IORING_OP_ASYNC_CANCEL` reaches
it there ([the two platforms](platforms.md)).

A read-side open of a fifo meets the first condition too, and the pool waits it
out another way. The worker opens the path non-blocking first. When that finds
a fifo, a helper thread opens it again in a blocking `open(2)`, which returns
when a writer opens the other end. The worker waits for the helper with the
stop pipe and the bound in view. A stop or an expired bound wakes the helper:
the worker opens the write end non-blocking and closes it at once. The first
descriptor is a reader, so that open succeeds, and every reader waiting in an
open of the fifo wakes with it. When the fifo's permissions refuse that open,
the worker answers at once and the helper waits on for a real writer, holding
its thread until one comes.

`Resolve` meets neither condition, and says so with `Bounds::uninterruptible`.
`getaddrinfo(3)` runs to the resolver's own end, so a cancel discards the
result without giving the worker thread back any sooner. Every use of that
constructor names the call that behaves this way.

A `File` operation on metadata, a stat or a rename for example, meets neither
condition. It opens nothing and makes one blocking call, so on the pool it takes
`Bounds::uninterruptible` as well. The calls that move a file's contents are port calls,
which a stop pipe reaches ([files](../../io/files.md)).

On the ring, `IORING_OP_ASYNC_CANCEL` reaches these operations too. One that
io-wq has not started answers `-ECANCELED` and makes no call. io-wq signals the
thread that runs a started one, which ends only a call that sleeps where a
signal reaches it. Any other call runs to its end, and its CQE carries its own
result. Either way the drain retires the entry, so a cancelled sync submits no
step after the one that answered.

Four `File` operations walk instead, and each step is one blocking call. `List`
reads a directory's entries, and a recursive `DeleteDir` removes a tree entry by
entry. A `Mkdir` with `parents` makes each missing ancestor, and `Canonicalize`
resolves a path one component at a time. A walk lasts as long as what it walks
is large, so it carries a stop pipe and checks it, without waiting, before each
step. A stopped walk answers `-ECANCELED`, and the steps it took stay done. The
check does not reach a step already inside its call, such as a `lstat(2)` on a
mount that has stopped answering. That step runs to its own end.

When the process is out of descriptors, `bounds` returns a `Bounds` with no
stop pipe, and the operation runs uncancellable. It still bounds itself by the
caller's `:timeout`, which `OpBound` enforces with the same poll
([how an operation is bounded](bounds.md)).

## Pinned by

- [io-cancel-releases.lisp](../../../tests/lang/io-cancel-releases.lisp), for
  the descriptor a cancelled operation gives back.
- [io-cancel-workers.lisp](../../../tests/impl/io-cancel-workers.lisp), which
  counts the workers.
- [ringopen.rs](../../../src/io/aio/tests/ringopen.rs), for an open the ring
  cancels while it waits for a fifo's reader.
- [retire.rs](../../../src/io/pending/tests/retire.rs) and
  [metaops.rs](../../../src/io/aio/tests/metaops.rs), for the directory a
  cancelled `file/mktempdir` made on either platform, and the descriptor a
  cancelled sync on the ring opened.
- [walkstop.rs](../../../src/io/threadpool/tests/walkstop.rs), for a walk a
  stop ends before a step, and
  [walkcancel.rs](../../../src/io/aio/tests/walkcancel.rs), for a cancel that
  reaches a walk on either platform.
