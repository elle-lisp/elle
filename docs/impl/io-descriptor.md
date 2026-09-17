# Descriptors and workers

<!-- audited: 2026-09-16 -->

What a `port/close` retires, how it wakes the operations still holding the
descriptor, and how the thread pool reuses a worker.

Up: [io/](../../src/io/AGENTS.md)

## Descriptor retirement

**A descriptor number is not reused while any submitted operation still
names it.** A worker resolves its fd at syscall entry, not at submit
time, so a number returned to the OS while a worker still holds it can be
handed to a new socket before that worker runs — and the worker then
reads the new socket, and its bytes go to a completion no fiber is
waiting for.

A descriptor is therefore **shared** rather than owned outright. A `Port`
holds its `OwnedFd` behind an `Rc`, and every `PendingOp::Port` filed
against that port takes a share of its own at submit (`Port::fd_share`).
The number goes back to the OS when the last share drops, which is the
moment the last operation naming it is retired. Nothing has to remember
anything for that to hold, and it holds however the port goes away:
`port/close`, a port dropped without one, or the release that frees the
regions of a fiber which terminated by a route the scheduler did not take.

`port/close` reports closed the moment it is asked, so Elle's semantics
are unchanged — the port stops answering, and what it gives up is its
share. The `fd_states` entry goes at the same moment: a remainder belongs
to the port that produced it, and that port is what the close ended.

A port is not the only thing that owns a descriptor an operation names.
`WatchNext` reads the inotify (Linux) or kqueue (macOS) descriptor an
`FsWatcher` owns, and `SigNext` reads the signalfd (Linux) or kqueue
descriptor a `SignalReceiver` owns. Neither external hands out a share,
and neither needs to: both are operands, so the entry's hold on their
region keeps the external itself alive, and a live external has not
dropped its `OwnedFd`. The number is the OS's again only once the last
operation naming it has let its hold go.

Pinned for the watcher by
`a_watcher_freed_with_its_fibers_regions_keeps_its_descriptor_number`
(`src/io/aio/tests/park.rs`).

Pinned by `tests/elle/io-cancel-releases.lisp`, by
`a_descriptor_share_holds_the_number_until_it_drops` (`src/port/tests.rs`)
for the share itself, and — for a port that goes with its fiber's regions
rather than through a close — by
`a_port_freed_with_its_fibers_regions_keeps_its_descriptor_number`
(`src/io/aio/tests/park.rs`).

## How a close wakes the operations it retires

A retired descriptor is only given back once its operations complete, so
the close must also make sure they DO complete — a worker parked on a
descriptor nobody will ever act on again holds its wait forever, and the
fiber behind it is never resumed. The close wakes each pending operation
on the port, by descriptor kind:

- **A connected stream socket** (`TcpStream`, `UnixStream`) is woken by
  `shutdown(2)`: the worker's poll reports the fd readable, its read
  returns zero bytes, and the fiber sees a clean EOF.
- **Everything else** — a listener, a datagram socket, a pipe — is woken
  through the operation's stop pipe, the same wake `io/cancel` uses.
  `shutdown(2)` cannot reach these: shutting down a LISTENING socket
  wakes a parked accept only on Linux (macOS and the BSDs return
  `ENOTCONN` and wake nothing), and an unconnected UDP socket or a pipe
  is not a connected socket on any platform.

Unlike `io/cancel`, the close does not mark the operation cancelled: the
worker's error completion flows back to the fiber, which resumes and can
exit cleanly. Pinned by `closing_a_listener_ends_its_parked_pool_accept`
(`src/io/aio/tests/net.rs`) and, end to end through two processes, by
`tests/elle/process-accept-close.lisp`.

## How many operations run at once

The OS decides. A pool operation runs on a worker thread, and a thread is
started whenever no idle worker is there to take the job, so the ceiling is
`RLIMIT_NPROC`, `kernel.threads-max` and the memory for the stacks — limits
the operator set, on a machine the runtime cannot survey.
`Builder::spawn` rather than `thread::spawn` is what makes deferring to
them possible: `thread::spawn` panics when the OS refuses, while a
`Builder` refusal becomes the error `io/submit` returns and the calling
fiber can handle.

Nothing caps the count, and nothing may. An operation can wait for an event
outside this process — an accept nobody connects to, a read whose peer never
writes — and such an operation ends only through its deadline or its stop
pipe. Under a cap the next submission would queue behind that wait, and the
fiber that would issue the write ending it is a fiber the cap is holding up.
So the crew grows to whatever is asked of it.

`io/workers` reports how many operations are submitted and not yet reaped
— the workers busy right now, not the ones parked waiting for work — and
`ev/report` carries it as `:workers`. That is a measurement, not a budget:
nothing consults it to decide whether a submission may proceed.

io_uring has no equivalent count. Its operations run in the kernel, so
`workers()` is zero there and the only limit is the 256-entry submission
queue, which drains as it is submitted.

## How a worker is reused

A worker that finishes an operation waits for another instead of exiting, so
the next submission costs a channel send rather than a thread. `WorkerPool`
(`threadpool/pool.rs`) is the crew and the handoffs that reach it.
What reuse buys is wall clock under contention: starting and tearing down a
thread costs kernel work that scales with how many elle processes are doing it
at once, while the operation itself costs the same either way.

Each parked worker posts a **handoff** — a channel of its own — and a
submission takes one out of the list and sends the job through it, or starts a
thread when the list is empty. A worker leaves only by withdrawing its own
handoff under the same lock, so a claimed worker is committed to the job it was
handed. That is what keeps the handover from becoming the cap the section above
forbids: a job never waits behind a parked operation, because a job is only
ever handed to a worker that is already waiting for one.

**A worker sleeps rather than spins, and that is measured.** The handoff is a
condition variable, not a channel, because a channel receiver spins before it
sleeps and this wait happens once per operation. Where there are more cores
than threads that spin is free and often saves the sleep; where there are
fewer, it burns the cores the rest of the program is waiting for. On a
three-core runner the channel cost the heaviest corpus files **twice the user
CPU** they cost with no pool at all — 2.4s → 5.5s on one of them — while the
same files on a thirty-two-core box were unchanged either way. That is the
whole reason this is a `Condvar` and a slot rather than four lines of
crossbeam, and why a machine with spare cores cannot measure it.

**The list is a stack.** A submission takes the worker that parked most
recently: it is the one still warm, and the workers the traffic no longer
reaches sit at the bottom and age out of the keepalive instead of being woken
in turn. This is a reasoned default rather than a measured win — the wake order
was not what the three-core runner was punishing — but it is the order the crew
is built on, so a test pins it.

A worker that parks for the keepalive without being handed a job retires, so a
program that stops doing I/O stops paying for threads. It retires only if it can
withdraw its own handoff. Finding nothing to withdraw means a submission took it
already and a job is on its way, so the worker waits on without a deadline
rather than leaving and stranding it. Under the stack, the workers that age out
this way are the ones the traffic no longer reaches.

The keepalive is the program's, not the process's: `*io-keepalive*` binds the
seconds, a scheduler reads that parameter when it builds its backend, and
`(io/backend :async k)` carries it to the hub. `nil` takes `DEFAULT_KEEPALIVE`,
which is where the default number and the two costs it trades off are written
down. `0` retires a worker as soon as it has nothing to do — the
counter-factual switch, one thread per operation, and what makes the difference
reuse buys measurable rather than asserted. Two schedulers in one process can
answer differently, so this is a parameter rather than a dialect.

Pinned by `a_backend_takes_the_keepalive_it_was_given`
(`src/io/aio/tests/backend.rs`) for the path from the argument to the crew, and
by `tests/elle/io.lisp` § "worker keepalive" for the parameter and the forms
`io/backend` accepts.

Dropping the backend drops every posted handoff, so each parked worker's wait
reports the disconnect, and it sets the flag that a worker still running an
operation reads when it goes to park. The crew winds down on both paths with
nothing to join.

A worker outlives the operations it runs, so what an operation does to its
thread must not survive it. Blocking every asynchronous signal is therefore
what a worker is started with rather than what each job does
(`mask_all_signals_on_this_thread`), and the one operation that needs a
different mask — the macOS `EVFILT_SIGNAL` read, which must be selectable for
delivery — puts back what it found
(`the_macos_signal_read_blocks_again_what_it_unblocked`,
`src/io/threadpool/tests/signals.rs`).

Pinned by `src/io/threadpool/tests/pool.rs`: a second submission runs on the
first one's thread, the next job goes to the worker that parked last, a parked
operation delays no other submission, an idle worker retires, and a zero
keepalive gives every operation its own thread.

