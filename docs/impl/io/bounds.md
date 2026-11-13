# How an operation is bounded

<!-- audited: 2026-11-11 -->

How the ring and the thread pool each hold an operation to its `:timeout` and its `:deadline`.

[I/O deadlines](../../io/timeout.md) owns what the two bounds mean. A request
carries both in its `Bound`, and every wait a platform arms asks the `Bound`
how long it may last at that moment (`Bound::next_wait`). A `connect` measures
its bound across its retries, because one connect is one operation however many
times the kernel makes the worker ask.

| Platform | Mechanism | An expired wait reports |
|----------|-----------|-------------------------|
| io_uring | a `LinkTimeout` SQE linked to the operation's SQE, and armed again on every resubmission | `ECANCELED` |
| thread pool | `OpBound` takes the descriptor non-blocking and waits for readiness in `poll(2)`, armed again after every transfer | `ETIMEDOUT` |

The completion reads both errnos as the `:timeout` error kind
(`is_timeout_errno` in [completion.rs](../../../src/io/completion.rs)).

## The ring

A `LinkTimeout` SQE follows the operation's SQE with the `IO_LINK` flag, so the
kernel cancels the operation when the timeout fires first. The operation's CQE
then carries `-ECANCELED`. The timeout's own CQE carries a tag in its high bit,
`id | (1 << 63)`, and the drain skips it.

The SQE holds only a pointer to its `Timespec`, and the kernel reads it during
the `io_uring_enter` that submits the SQE. So the timespec lives until that
submit returns, in `submit_linked` and in `push_resubmit` alike.

An operation that needs several SQEs is armed again on each, for what the bound
allows at that moment. `PendingOp::Port.bound` carries the bound for that.

## The thread pool

`OpBound` ([opbound.rs](../../../src/io/threadpool/opbound.rs)) holds the
descriptor non-blocking for the operation's lifetime. It waits in `poll(2)` for
readiness, for the caller's bound, or for the stop pipe
([cancelling an operation](cancel.md)).

The bound holds for every kind of descriptor, which is why the worker owns the
wait rather than handing it to the descriptor. `SO_RCVTIMEO` and `SO_SNDTIMEO`
bound a socket, and a pipe, a fifo and a tty refuse them. A reader that stops
reading fills a pipe exactly as it fills a socket, and the write that follows
would then park in the kernel forever. `poll(2)` takes every descriptor, so
`OpBound` bounds every descriptor.

Non-blocking mode is what makes the poll enough. A blocking syscall can park
again after a poll reports the descriptor ready, while a non-blocking one
answers `EAGAIN` and hands the wait back to `OpBound`.

`O_NONBLOCK` lives on the open file description, so two operations on one
descriptor share it. `OpBound` counts them: the first sets the flag and records
what it found, and the last puts that back. Every read and write loop treats
`EAGAIN` as a readiness wait whether or not it asked for a timeout. So an
untimed operation that meets a descriptor another operation made non-blocking
waits rather than fails.

## Pinned by

- [port-write-timeout.lisp](../../../tests/lang/port-write-timeout.lisp) and
  [port-read-timeout.lisp](../../../tests/lang/port-read-timeout.lisp), each
  run on each platform, against a socket peer and a pipe peer.
- [net-wait-timeout.lisp](../../../tests/lang/net-wait-timeout.lisp), for the
  calls that wait for a peer.
- `a_pool_connect_reports_its_own_deadline_as_a_timeout`
  ([netend.rs](../../../src/io/aio/tests/netend.rs)), for the connect, whose
  stall needs a listener backlog an Elle script cannot set.
