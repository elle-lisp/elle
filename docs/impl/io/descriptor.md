# Descriptors and workers

<!-- audited: 2026-11-11 -->

What a `port/close` retires, how it wakes the operations still holding the descriptor, and how the thread pool reuses a worker.

## Descriptor retirement

**A descriptor number is not reused while any submitted operation still names
it.** A worker resolves its descriptor when it enters the syscall, not at
submit time. A number returned to the OS while a worker still holds it can be
handed to a new socket before that worker runs. The worker then reads the new
socket, and its bytes go to a completion no fiber is waiting for.

So a descriptor is **shared** rather than owned outright. A `Port` holds its
`OwnedFd` behind an `Rc`, and every `PendingOp::Port` filed against that port
takes a share of its own at submit (`Port::fd_share`). The number goes back to
the OS when the last share drops, which is the moment the last operation naming
it is retired. Nothing has to remember anything for that to hold. It holds
however the port goes away: `port/close`, a port dropped without one, or the
release that frees the regions of a fiber which ended by a route the scheduler
did not take.

`port/close` reports closed the moment it is asked, so a program sees a close:
the port stops answering, and what it gives up is its share. The port's
`fd_states` entry goes at the same moment, because a remainder belongs to the
port that produced it, and the close ended that port.

A port is not the only thing that owns a descriptor an operation names.
`WatchNext` reads the inotify (Linux) or kqueue (macOS) descriptor an
`FsWatcher` owns, and `SigNext` reads the signalfd (Linux) or kqueue descriptor
a `SignalReceiver` owns. Neither external hands out a share, and neither needs
to. Both are operands, so the entry's hold on their region keeps the external
itself alive, and a live external has not dropped its `OwnedFd`. The number is
the OS's again only once the last operation naming it has let its hold go.

Pinned by:

- [io-cancel-releases.lisp](../../../tests/lang/io-cancel-releases.lisp).
- `a_descriptor_share_holds_the_number_until_it_drops`
  ([tests.rs](../../../src/port/tests.rs)), for the share itself.
- `a_port_freed_with_its_fibers_regions_keeps_its_descriptor_number` and
  `a_watcher_freed_with_its_fibers_regions_keeps_its_descriptor_number`
  ([descriptor.rs](../../../src/io/aio/tests/descriptor.rs)), for a port and a
  watcher that go with their fiber's regions rather than through a close.

## How a close wakes the operations it retires

A retired descriptor goes back only once its operations complete, so the close
must also make sure they do complete. A worker parked on a descriptor nobody
will act on again holds its wait forever, and the fiber behind it never
resumes. The close wakes each pending operation on the port by the kind of
descriptor:

- **A connected stream socket** (`TcpStream`, `UnixStream`) is woken by
  `shutdown(2)`. The worker's poll reports the descriptor readable, its read
  returns zero bytes, and the fiber sees a clean end of stream.
- **Everything else**, a listener, a datagram socket or a pipe, is woken
  through the operation's stop pipe, the wake `io/cancel` uses
  ([cancelling an operation](cancel.md)). `shutdown(2)` cannot reach these.
  Shutting down a listening socket wakes a parked accept only on Linux, while
  macOS and the BSDs return `ENOTCONN` and wake nothing. An unconnected UDP
  socket or a pipe is not a connected socket on any platform.

Unlike `io/cancel`, the close does not mark the operation cancelled. The
worker's error completion flows back to the fiber, which resumes and can end
cleanly. Pinned by `closing_a_listener_ends_its_parked_pool_accept`
([netend.rs](../../../src/io/aio/tests/netend.rs)), and end to end through two
processes by
[process-accept-close.lisp](../../../tests/lang/process-accept-close.lisp).

## How many operations run at once

The OS decides. A pool operation runs on a worker thread, and a thread starts
whenever no idle worker is there to take the job. So the ceiling is
`RLIMIT_NPROC`, `kernel.threads-max` and the memory for the stacks: limits the
operator set, on a machine the runtime cannot survey. The pool starts a thread
with `Builder::spawn` rather than `thread::spawn`, because `thread::spawn`
panics when the OS refuses. A `Builder` refusal becomes the error `io/submit`
returns, which the calling fiber can handle.

Nothing caps the count, and nothing may. An operation can wait for an event
outside this process, such as an accept nobody connects to or a read whose peer
never writes. Such an operation ends only through its deadline or its stop
pipe. Under a cap the next submission would queue behind that wait, and the
fiber that would issue the write that ends it could be the one the cap holds
up. So the crew grows to whatever is asked of it.

`io/workers` reports how many operations are submitted and not yet reaped: the
workers busy now, not the ones parked waiting for work. `ev/report` carries it
as `:workers`. It is a measurement, not a budget, and nothing reads it to
decide whether a submission may go ahead.

On the ring the kernel runs most operations, and they hold no worker. There
`workers()` counts only the work that [runs on the pool](platforms.md) on both
platforms. The ring itself is limited by its 256-entry submission queue, which
drains as it is submitted.

## How a worker is reused

A worker that finishes an operation waits for another instead of exiting, so
the next submission costs a handoff rather than a thread. `WorkerPool`
([pool.rs](../../../src/io/threadpool/pool.rs)) is the crew and the handoffs
that reach it. Reuse saves wall-clock time under contention: starting and
ending a thread costs kernel work that grows with how many processes do it at
once, while the operation itself costs the same either way.

Each parked worker posts a **handoff**, a slot of its own. A submission takes
one out of the list and sends the job through it, or starts a thread when the
list is empty. A worker leaves only by withdrawing its own handoff under the
same lock, so a claimed worker is committed to the job it was handed. That
keeps the handover from becoming the cap the section above forbids: a job is
only ever handed to a worker already waiting for one, so it never waits behind
a parked operation.

**A worker sleeps rather than spins.** The handoff is a condition variable, not
a channel, because a channel receiver spins before it sleeps, and this wait
happens once per operation. Where a machine has more cores than threads, the
spin is free and often saves the sleep. Where it has fewer, the spin burns the
cores the rest of the program is waiting for.

**The list is a stack.** A submission takes the worker that parked most
recently, because it is the one still warm. The workers the traffic no longer
reaches sit at the bottom and age out of the keepalive rather than being woken
in turn. A test pins the order, because the crew is built on it.

A worker that parks for the keepalive without being handed a job retires, so a
program that stops doing I/O stops paying for threads. It retires only if it
can withdraw its own handoff. Finding nothing to withdraw means a submission
took it already and a job is on its way, so the worker waits on without a
deadline rather than leave and strand that job.

The keepalive is the program's, not the process's. A program binds the seconds
to `*io-keepalive*` ([dynamic parameters](../../parameters.md)). A scheduler
reads that parameter when it builds its backend, and `(io/backend :async k)`
carries it to the hub. `nil` takes `DEFAULT_KEEPALIVE`, whose docstring names
the default and the two costs it trades. `0` retires a worker as soon as it has
nothing to do, one thread per operation, which is what makes the saving reuse
buys measurable. Two schedulers in one process can choose differently, so this
is a parameter rather than a setting of the process.

Pinned by `a_backend_takes_the_keepalive_it_was_given`
([backend.rs](../../../src/io/aio/tests/backend.rs)) for the path from the
argument to the crew, and by [io.lisp](../../../tests/lang/io.lisp) for the
parameter and the forms `io/backend` accepts.

Dropping the backend drops every posted handoff, so each parked worker's wait
reports the disconnect. It also sets the flag that a worker still running an
operation reads when it goes to park. The crew winds down on both paths with
nothing to join.

A worker outlives the operations it runs, so what an operation does to its
thread must not survive it. A worker therefore starts with every asynchronous
signal blocked (`mask_all_signals_on_this_thread`), rather than each job
blocking them. The one operation that needs a different mask, the macOS
`EVFILT_SIGNAL` read, which must be selectable for delivery, puts back what it
found (`the_macos_signal_read_blocks_again_what_it_unblocked`,
[signals.rs](../../../src/io/threadpool/tests/signals.rs)).

Pinned by [pool.rs](../../../src/io/threadpool/tests/pool.rs):

- a second submission runs on the first one's thread;
- the next job goes to the worker that parked last;
- a parked operation delays no other submission;
- an idle worker retires;
- a zero keepalive gives every operation its own thread.
