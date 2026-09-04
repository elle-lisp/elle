# I/O Module

## Purpose

The `io` module contains I/O request types and backends for Elle's
scheduler-based I/O system. Stream primitives build `IoRequest` values
and yield them via `SIG_IO`. The scheduler catches `SIG_IO` and dispatches
to a backend for execution.

## Modules

| Module | Responsibility |
|--------|----------------|
| `types.rs` | Shared types: `PortKey`, `FdState` — used by both backends |
| `pool.rs` | `BufferPool`, `BufferHandle` — pinned buffer management for async I/O |
| `pending.rs` | `PendingOp` enum — in-flight async operation tracking, one variant per operation shape — and `PendingTable`, the backend's set of them plus the ids no fiber will read. `take` answers "does anybody want this?" once, for both backends. |
| `aio.rs` | `AsyncBackend` — async I/O with io_uring (Linux) or thread-pool fallback |
| `request.rs` | `IoRequest` and `IoOp` types — typed I/O request descriptors |
| `completion.rs` | `process_raw_completion` — converts raw CQE/thread results to `Completion` |
| `sigfd.rs` | `SignalReceiver` — POSIX signalfd (Linux) or kqueue+EVFILT_SIGNAL (macOS) external for `os/sig-watch`; also the worker-thread mask helper `mask_all_signals_on_this_thread` |
| `sigmap.rs` | Shared keyword↔signum mapping; `resolve(value, ctx)` parses a `:sigterm`/integer Value to libc signum |
| `sockaddr.rs` | Sockaddr construction, formatting, parsing — single source of truth |
| `threadpool.rs` | `CompletionHub` (the one shared completion channel), `RawCompletion`, `PoolOp`, `PoolCompletion`, `StdinThread` — typed thread-pool I/O. Every spawned worker calls `crate::io::sigfd::mask_all_signals_on_this_thread()` first so the kernel never selects it as a POSIX-signal delivery target. |
| `threadpool/opbound.rs` | `Bounds` and `OpBound` — the declared and the live half of one operation's bound — plus `Wake`, the stop pipe, `take_when_ready` and `pace_retry`. |
| `threadpool/submitop.rs` | `CompletionHub::submit`: hand the operation to a worker, run it, publish the result. The `match` there names each operation's runner and the descriptor its bound watches. |
| `threadpool/pool.rs` | `WorkerPool`, `Crew` and `Job` — the parked workers' handoffs, and the choice between handing a job to one of them and starting a thread. See § "How a worker is reused". |
| `threadpool/{stream,net,event,child,open}.rs` | The runners, grouped by what they wait on: byte streams, sockets, event descriptors (inotify / kqueue / signalfd), a child's exit, a file open. |
| `uring.rs` | io_uring SQE submission and CQE processing (Linux only). The standing `POLL_ADD` on the hub's bridge eventfd carries the `EVENTFD_USER_DATA` sentinel; `drain_cqes` reports it as `eventfd_fired` and the wait/poll path clears + re-arms it. |
| `eventfd.rs` | Bridge eventfd helpers — `create`/`signal`/`drain` (Linux only). One definition of each eventfd syscall, shared by the io_uring bridge and `primitives::chan`'s wake fd. |


## Data Flow

Sync path:
```
Stream primitive → (SIG_IO, IoRequest) → Scheduler → io/submit → AsyncBackend → OS
```

Async path:
```
Stream primitive → (SIG_IO, IoRequest) → Scheduler → io/submit → AsyncBackend → OS (async)
                                                    ← io/wait  ← completions ← OS
```

## Key Types

### IoOp

Enum of I/O operations (16 variants):

**Stream operations:** `ReadLine`, `Read { count }`, `ReadAll`, `Write { data }`, `Flush`

**File position:** `Seek { offset: i64, whence: i32 }`, `Tell`

**Network operations:** `Accept`, `Connect { addr }`, `SendTo { addr, port_num, data }`, `RecvFrom { count, result }`, `Shutdown { how }`

`RecvFrom` pre-allocates its `{:data :addr :port}` result struct on the
**requesting fiber's heap** (`prim_udp_recv_from`) and the completion fills it
in place — the iovec receives the payload zero-copy into `:data`, and `:addr`/
`:port` are stamped into the struct's slots (`set_struct_field_in_place`). This
mirrors `Read`/`Accept`: nothing is instantiated on the scheduler's heap at
completion, so the value the fiber resumes with has no cross-heap reference (the
"datagram arrives zeroed" arena-lifetime bug).

**Timer:** `Sleep { duration }`

**Subprocess operations:** `Spawn { program, args, env, cwd, stdin, stdout, stderr }`, `ProcessWait`

**Filesystem watch:** `WatchNext` — portless; the `FsWatcher` external lives in `IoRequest.port`. Read from the inotify (Linux) or kqueue (macOS) fd.

**POSIX signal reception:** `SigNext` — portless; the `SignalReceiver` external lives in `IoRequest.port`. Reads from the signalfd (Linux) / kqueue fd (macOS) opened by `os/sig-watch`. Completion is an array of structs `{:signal :sigterm :sender-pid n :sender-uid n :code n :count n}`. See `docs/posix-signals.md` for the mask policy that backs this op.

**Background task:** `Task(TaskFn)` — run an arbitrary closure on a background thread. `TaskFn` wraps a `FnOnce() -> (i32, Vec<u8>) + Send` in `RefCell<Option<...>>` for take-once semantics. Non-negative result_code = success (data returned as `Value::bytes`), negative = error (data is UTF-8 error message). `IoRequest::task()` is the convenience constructor.

### PortKind

Enum of port types (10 variants):

**File-based:** `File`, `Stdin`, `Stdout`, `Stderr`

**Network:** `TcpListener`, `TcpStream`, `UdpSocket`, `UnixListener`, `UnixStream`

**Subprocess:** `Pipe` — represents a subprocess stdio fd (stdin, stdout, or stderr). Display format: `#<port:pipe "pid:1234:stdout" :read :binary>`. All exhaustive matches on `PortKind` must be updated when adding new variants.

### ProcessHandle

Struct representing a running subprocess. Fields:
- `pid: u32` — process ID
- `child: RefCell<Child>` — the spawned child, kept so an unreaped one can be reaped on drop
- `exit: ExitRecord` — where the child's exit status is kept once somebody reaps it. See § "A reap is never wasted"

Methods:
- `new(pid, child) → ProcessHandle` — create from spawned child process
- `pid() → u32` — get process ID
- `exit() → &ExitRecord` — the record, for the waiters and the operations that clone it
- `Drop` impl — calls `try_wait()` on a child nothing has reaped, to reap zombies

### ExitRecord

The one place a child's exit status is kept (`src/io/request/process.rs`).
`Arc<Mutex<Option<i32>>>` behind a newtype, so a pool worker on another thread
and the scheduler thread write the same record.

- `new() → ExitRecord` — an unreaped child's record
- `status() → Option<i32>` — the status this process is holding, if any
- `keep(code)` — record a status somebody else's reap produced (the kernel's `waitid`). The first status wins; a child is reaped once
- `reap(pid) → Reap` — `waitpid(pid, .., WNOHANG)` under the record's lock, answering `Exited`, `Running` or `Failed(errno)`

`Reap::Exited` covers both "this ask reaped the child" and "the record already
held it": holding the lock across the syscall is what makes the ask and the
record one step. `exit_code_from_wait_status` and `exit_code_from_siginfo` are
the two decodes — a `waitpid` status word and a kernel-filled `siginfo_t` —
and both live here, beside the record they feed.

### PendingOp

What one in-flight operation's completion needs, one variant per operation
shape. Every variant carries a `BufferHandle`; the rest is what that operation
alone must remember:

- `Port { op, port_key, port, descriptor, buffer_handle, listener_kind, filled, timeout }` — operation on an existing port (stream I/O, accept, datagram, shutdown). `descriptor` is this operation's share of the number it names — see § "Descriptor retirement". `listener_kind` is `Some(PortKind)` for Accept only.
- `Connect { addr, buffer_handle, connect_fd, port }` — creates a new port on completion. `connect_fd` starts as `Some(fd)` for io_uring (pre-created socket) or `None` for thread pool (set on completion).
- `Open { path, buffer_handle, port }` — creates a new port on completion; `path` is kept for the error message.
- `Sleep { buffer_handle }` — portless timer.
- `ProcessWait { buffer_handle, handle_val, siginfo, exit }` — waiting for subprocess exit via IORING_OP_WAITID. `siginfo` is a heap-allocated `siginfo_t` filled by the kernel; released in completion processing. Null on the thread-pool path, where the worker reports the exit code itself. `exit` is a clone of the handle's `ExitRecord`, so the entry can keep a reaped status without dereferencing `handle_val` — see § "A reap is never wasted".
- `Task { buffer_handle }` — background task running on thread pool.
- `Resolve { buffer_handle }` — getaddrinfo(3) on the thread pool.
- `WatchNext { watcher, buffer_handle }` / `SigNext { receiver, buffer_handle }` — a read on the inotify / signalfd descriptor the external owns. Both are operands, so the entry's hold keeps the external — and therefore the descriptor it owns — for the read's lifetime; see § "Descriptor retirement".
- `PollFd { buffer_handle }` — readiness wait on a bare descriptor.
- `ChanSelectPark { buffer_handle, guard }` — readiness wait on a `chan/wait-ready` wake fd. The guard owns the fd(s) and the wake-list registrations, so dropping this entry deregisters exactly once.

### PoolOp / PoolCompletion / RawCompletion / CompletionHub

Typed thread-pool submission and completion:

- `PoolOp` — one variant per operation the pool runs. Each carries exactly the data that operation needs (fd, buffers, addresses, or closures) and nothing about waiting: a typed submission.
- `Bounds` — how long an operation may wait and how `io/cancel` ends it, passed alongside the `PoolOp` to every `CompletionHub::submit`. Three constructors, and a submission must pick one: `CompletionHub::bounds(id, timeout)` pairs the caller's deadline with a fresh stop pipe, `Bounds::prompt()` says the syscalls wait on nothing outside this process, and `Bounds::uninterruptible()` says the syscall cannot be stopped once entered. Because the bound is an argument rather than a field, a variant cannot forget it. The `Bounds` own the stop pipe's read end and close it with themselves, so a submission no worker runs — a refused `Builder::spawn`, a path the kernel rejects — disposes of the pipe by being dropped.
- `OpBound` — what a worker runs under: it holds the descriptor non-blocking for the operation's lifetime and turns the declared `Bounds` into waits. `OpBound::new(fd, ..)` for an operation that reads or writes `fd`, `OpBound::watching(fd, ..)` for one that only polls a descriptor somebody else owns, `OpBound::detached(..)` for one with no descriptor at all.
- `PoolCompletion { id, kind, result_code, data }` — typed completion from a thread-pool worker. `kind` is the `OpKind` the worker ran, checked against the entry the id resolves through — see § "One id, one operation".
- `RawCompletion` — `Pool(PoolCompletion)` | `Stdin(StdinCompletion)`. The single
  shape every background worker ships through the hub. A worker cannot build a
  cooked `Completion` (the cook fns need main-thread `pending`/`fd_states`/
  `buffer_pool`/`origin_heap`), so it sends its raw result; the receiver matches
  once and dispatches to `pool_to_completion` / `stdin_to_completion`.
- `CompletionHub { sender, receiver, in_flight, eventfd, stops, pool }` — the **one** completion
  channel all background work feeds: every thread-pool worker and the stdin worker
  holds a `Sender<RawCompletion>` clone. Collapsing the former platform-pool,
  network-pool, and stdin channels into one means the scheduler's blocking wait
  reads exactly one source: a crossbeam `recv()` registers-before-sleeps on the
  sole channel, so there is nothing to exclude and no wakeup to miss. `in_flight`
  is the combined count of submitted-but-unreaped worker ops (pool + stdin): +1 per
  worker submit, −1 once per `RawCompletion` reaped at the single drain site (a
  cancelled op's reaped completion still decrements; `io/cancel` must not also
  decrement). `eventfd` is the Linux/uring bridge fd (`None` on the pool-only
  platforms) a worker writes after `send` so the ring's single wait observes the
  edge. `stops` is the write end of each submitted operation's stop pipe, by id.
  `pool` is the crew that runs the operations — see § "How a worker is reused".

### ConnectAddr

Enum: `Tcp { addr, port }` or `Unix { path }`. `Tcp.addr` is a **parsed
`std::net::IpAddr`** — connect is IP-only at the backend. The `tcp/connect-ip`
primitive parses the IP and builds this; hostname resolution is the stdlib
`tcp/connect` wrapper's job (`sys/resolve` → `tcp/connect-ip` per address), so
the backend never runs a blocking getaddrinfo fallback.

### IoRequest

Struct: `{ op: IoOp, port: Value, timeout: Option<Duration> }`.

### Completion

Returned to Elle as struct: `{:id n :value v :error nil}` (success) or `{:id n :value nil :error e}` (failure).

## Sockaddr Module

`sockaddr.rs` provides the single source of truth for socket address operations:

- `build_inet(addr) → (Vec<u8>, socklen_t)` — build sockaddr_in/in6 as bytes
- `build_unix(path) → Result<(sockaddr_un, socklen_t), String>` — build sockaddr_un with abstract socket support
- `format(storage, len) → String` — format as `"ip:port"`, `"[ipv6]:port"`, or unix path
- `parse(storage, len) → (String, u16)` — parse to (addr_string, port)
- `peer_address(fd) → String` — getpeername + format
- `local_address(fd) → String` — getsockname + format

All formatting uses `std::net::Ipv4Addr`/`Ipv6Addr` for canonical output (proper IPv6 shortening).

## Primitives

| Primitive | Signal | Purpose |
|-----------|--------|---------|
| `io-request?` | silent | Check if value is an I/O request |
| `io-backend?` | silent | Check if value is an I/O backend |
| `io/backend` | errors | Create an I/O backend (`:sync` or `:async`) |

| `io/submit` | errors | Submit async I/O request, return submission ID |
| `io/reap` | errors | Non-blocking poll for completions (returns array) |
| `io/wait` | errors | Blocking wait for completions with timeout (returns array) |
| `io/cancel` | errors | Cancel a pending async I/O operation by submission ID |
| `ev/sleep` | error, yield, io | Async sleep (in `primitives/time.rs`) |

## Timeout Handling

**Sync backend:** Post-hoc check after blocking syscall. Not preemptive.

**Async backend (io_uring):** Linked timeout SQEs provide true preemptive timeout for all operations (stream, network, and timer). A `LinkTimeout` SQE is submitted immediately after the operation SQE with the `IO_LINK` flag. If the timeout fires first, the kernel cancels the linked operation. The operation CQE has `result = -ECANCELED` (errno 125). The timeout CQE is identified by a high-bit tag (`id | (1 << 63)`) and skipped during completion processing.

**Thread-pool fallback:** `OpBound` (`threadpool/opbound.rs`) takes the
descriptor non-blocking and waits in `poll(2)` for readiness, for the
caller's `:timeout`, or for the stop pipe. See § "Operation timeouts" for
the mechanism and § "The stop pipe" for the cancellation half.

## I/O Cancellation

`io/cancel` has two halves. **Asking the operation to stop** is
platform-specific: io_uring takes `IORING_OP_ASYNC_CANCEL` (the cancel SQE's own
CQE carries the high-bit tag, same as a timeout CQE, and `drain_cqes` skips it;
the operation's own CQE arrives with `result = -ECANCELED`), while a pool worker
is asked through its stop pipe. **Marking the submission** is shared: both
platforms record the id in `PendingTable`, and both retire the entry when its
completion arrives instead of building a result from it.

Used by `do-shutdown` in stdlib to cancel pending I/O before aborting/cancelling
fibers, and by `ev/timeout`, which cancels whichever of the body and the timer
lost.

### What cancellation promises, on either backend

Three things hold however the operation ends.

- **The submission is accounted for.** `cancel` marks the id and leaves the
  `pending` entry in place. The operation's completion still arrives, still
  finds its entry, and — on the pool — still decrements `in_flight`. Removing
  the entry at the cancel would strand the submission: the worker's thread would
  never be accounted for again, and cancellation is a path `ev/timeout` takes on
  every call.
- **No completion is delivered.** `PendingTable::take` reports a cancelled
  submission as such, and its entry is retired rather than cooked — the pooled
  buffer released, a descriptor the completion would have wrapped in a port
  closed, a process wait's `siginfo_t` reclaimed. Nothing is left for a fiber,
  and nothing needs to be: every `io/cancel` caller in the scheduler
  (`complete-fiber`, `handle-abort`, `handle-io-forward-cancel`, `do-shutdown`)
  drops its own record of the submission first, so the id is marked precisely
  when there is no longer a reader. Cooking it anyway would read what the
  operation held — the port `Value`, the process handle, the fiber's
  pre-allocated read buffer — after the finished fiber's release freed the
  regions those live in. Pinned by
  `a_cancelled_operation_delivers_no_completion_on_either_backend`
  (`src/io/aio/tests/park.rs`), which holds both backends to the one answer.
- **The descriptor outlives the operation.** See "Descriptor retirement" below.

Backend teardown marks everything in flight the same way (`PendingTable::
cancel_all`, from `quiesce_pending`), for the same reason at a larger scale: the
heap those values live on may already be gone.

A cancel for an id that is no longer in flight marks nothing. The operation
completed and its result already reached the fiber that asked; a mark left
behind would meet a later submission.

### A reap is never wasted

One operation cannot honour "no completion is delivered" on its own terms.
`waitpid(2)` and `IORING_OP_WAITID` **consume** what they report: the kernel
hands a child's exit status over once, and the child is gone. A wait that is
cancelled just after the kernel handed the status over has already spent it, and
the promise above then throws that status away. The next `subprocess/wait` on
that child finds no child and reports `ECHILD` for a status this process took.

So the status does not travel in the completion alone. `ExitRecord` is where
whoever reaps puts it, and every `ProcessHandle` holds one. The pending entry
and the pool operation each carry a clone, so a write reaches no heap value: a
teardown drain keeps the status without dereferencing a handle whose region may
already be gone.

The alternative was to narrow the window rather than close it — check the stop
pipe immediately before each `waitpid` instead of only between them. The check
and the syscall cannot be made atomic, so a cancel landing between them still
reaps, and the guarantee stays unstatable.

The pool worker reaps **under the record's lock**, which makes the ask and the
answer one step: `ExitRecord::reap` returns the status a previous reap left
whenever there is one, so a second waiter can never see the gap between another
worker's `waitpid` and its write. The ring has that gap closed for it — the
kernel reaps, and the status is read off the `siginfo_t` where the CQE is
processed, whether the entry is cooked or retired (`PendingOp::retire`). A
`waitid` that lost the race reports `ECHILD` and answers from the record
instead; the winner reaped first, so its CQE precedes the loser's in the ring
and the record is already there.

`submit_process_wait` reads the record before it submits anything. A child's
exit status is delivered to a waiter, or held until one asks — which also
covers a child legitimately waited on twice.

The record is also what says whether a **pid still names this child**, which is
the other question a reap settles. The kernel returns a reaped pid to the pool
and hands it out again, so `subprocess/kill` reads the record before it makes a
`kill(2)`: a handle holding a status has no child of its own left, and the call
sends nothing rather than signalling whatever holds that number now. The two
readings of the record are the same fact — the child is gone — asked by a
waiter and by a killer. `docs/io.md` § "Killing a child that may already be
gone" carries the argument and the answers the primitive gives, and
`a_kill_on_a_reaped_child_sends_no_signal`
(`src/primitives/subprocess/tests.rs`) is the pin — a handle built over a pid
that names somebody else, since a test cannot recycle a pid on demand.

Each mechanism is pinned on its own, because each fails on its own.
`a_stopped_wait_that_reaped_the_child_keeps_its_status` and
`a_wait_on_an_already_reaped_child_answers_from_the_record`
(`src/io/threadpool/tests/process.rs`) hold the worker to reaping through the
record. In `src/io/aio/tests/process.rs`,
`a_cancelled_wait_that_reaped_the_child_answers_the_next_wait` runs the whole
path on the pool and its `_uring_` twin runs it on the ring (where the retire is
what keeps the status), `a_wait_on_a_held_status_files_no_operation` holds the
submit fast path to issuing nothing, and
`a_wait_that_finds_no_child_answers_from_the_record` builds the loser's `ECHILD`
at the entry, which no test can make two waits collide to produce.

### A submitted operation holds the values its completion reads

`PendingOp` holds `Value`s the completion dereferences when it assembles a
result: the port an operation names, the buffer or result struct the caller
reserved, the payload a write hands over, the process handle, the watcher, the
receiver. None of them are the operation's own allocations. Each was born in a
region of the fiber that asked, and the pending table is runtime-side state that
no free-time cascade reaches — the same position a message in a channel buffer
is in (`docs/impl/region/effects.md` § `Sends`).

So the entry **retains** each of these regions when it is filed, and lets go
when the entry is disposed. That is the seam-counted reference the send/receive
pair uses, for the reason it uses one: nothing else counts a reference held by
state outside the region system, so the seam counts its own.
`PendingOp::operands` is the list, and `OperandHold` is the reference.

The **fiber that asked** is held on the same terms, and it is the one held value
read on every drain rather than once at the completion: the sweep below reads its
status each time it runs. So it is the last value that may be assumed present —
the check that exists to notice a fiber is gone cannot itself be the thing that
reads that fiber after it went.

The hold **travels with the operation**. `take` hands it out alongside the
entry's `PendingOp`, so the completion reads the operands under it, and it is
let go once the result is built rather than when the entry leaves the table. A
read that needs another syscall carries its hold back in through `restore`
rather than releasing one and taking another. Releasing at the take instead
would free the port under the very assembly that reads it, whenever the entry's
hold is the last reference — which is exactly the case this exists for.

This is what makes assembling a completion safe by construction. A variant that
gains a value field and does not name it in `operands` still loses that, which
is why the match there is exhaustive over both `PendingOp` and `PortOp`.

A payload a write already copied at submit is listed too, though no completion
reads it back: `operands` is one list, and holding a value nobody will read
costs a reference until the operation ends.

Pinned by `a_submitted_operations_operands_outlive_the_fiber_that_asked`
(`src/io/aio/tests/park.rs`) and, for the fiber itself, by
`a_held_fiber_survives_the_release_of_its_region` (`src/io/pending.rs`).

### A hold retains what reclamation listens to

A retain on an `Owned` region is inert: that region is reclaimed by its owner's
subtree drop however many references point at it
(docs/impl/region/ownership.md). So the hold does not retain the operand's own
region — it retains the operand's **reclamation root**, the ancestor whose count
the subtree's fate hangs on. For a `Counted` operand the root is the region
itself and nothing changes; for an `Owned` one the root is what a count can
still reach. `FiberHeap::reclaim_root` is the walk, and `release` lets go of the
root it recorded, not of the region it started from.

An operand really does arrive `Owned`. A connection accepted inside a process
and handed to a per-connection `ev/spawn` is adopted into that activation's
subtree, and `handle-io-forward` then submits a write on it for the child
scheduler. `tests/elle/process-io.lisp` § 10 is that program, and the retain on
the port's own region there holds nothing at all.

Retaining the root keeps the whole subtree for the operation's lifetime, which
is more than the operand needs. That is the same trade the list of operands
already makes for a write's copied payload: one seam, one rule, and a reference
costs until the operation ends.

Pinned by `an_owned_operand_is_held_through_its_reclamation_root`
(`src/io/pending.rs`).

### A hold is let go while its store is still there

A release names a region in a store, so every release must run before that store
tears its regions down. `AsyncBackend::quiesce` is the release
(`PendingTable::release_holds`), and a heap runs it on every io-backend it still
carries before `RegionStore::teardown_all` — `FiberHeap::quiesce_io_backends`,
called from both `FiberHeap::drop` and `FiberHeap::clear`.

A backend reaches that teardown whenever the program that made it ends without
letting it go: a top-level `(io/backend :async)`, the scheduler's own backend,
and every value on the full-module WASM tier, which reclaims no region while it
runs. The sweep frees regions in id order rather than lifetime order, so a
backend destructor running inside it lets go of regions the same sweep may
already have freed — a phantom-region panic under debug assertions, a double
free without them. Draining first, while every region is still there, leaves
that destructor a hold that reaches nothing.

The same order is what lets a drain assemble a completion at teardown: the
values it reads are still allocated. That is why the WASM tier drained here
before there was a hold to release, and the rule now covers both.

Pinned by `a_stranded_backend_lets_go_before_its_heap_tears_down`
(`src/io/aio/tests/backend.rs`).

### An operation whose fiber is gone has no reader

A cancel is something a caller must remember to issue, and one caller cannot: a
fiber that terminates by a path the scheduler did not route. `fiber/cancel`
leaves such a fiber `:error` with its operation still submitted. Nothing marks
the id, and the scheduler finds out when it next looks at that fiber — which is
after the completion has been assembled, because assembling happens inside
`io/wait`.

So the reader-gone question is not asked of the canceller alone. Every entry
records **the fiber that asked**, which `io/submit` is handed at the call site
(`src/stdlib.lisp`), and a completion asks that fiber what became of it. A fiber
in a terminal state — `:dead` or `:error` — is one no result can reach, so the
entry is retired unread, exactly as a cancelled one is.

The fiber is a sound thing to ask because the entry holds it — see above — so it
is there to be asked for as long as the operation is.

**Why the entry withholds rather than assembling and letting the scheduler
drop the result.** The scheduler does drop it — `process-completions` delivers
only to a fiber `get-completion` reports as still running. But a result
assembled for a fiber that has gone holds values whose only remaining reference
is this entry's hold, and disposing of the entry is what lets that hold go.
Withholding keeps one fact in one place: nothing is built, so nothing outlives
the hold.

Unlike a cancelled operation, this one **answers**. A cancel is issued by a
caller that has already dropped its record of the submission, so silence leaves
nothing behind. Nobody dropped this id. The scheduler still pairs it with the
fiber that asked, and retires that pairing only on a completion, so silence here
would leave the event loop waiting on an operation that already finished. The
answer is an error built from nothing the entry held; the scheduler retires the
pairing and drops the error, because the fiber it would have gone to is what
went away.

A submission made on behalf of no fiber is never withheld. `handle-io-forward`
submits for a child scheduler, whose reader is a queue and a wake box rather
than a fiber in this one, and `io/cancel` through `handle-io-forward-cancel` is
how that reader lets go.

Pinned by `a_completion_is_withheld_when_the_fiber_that_asked_is_gone`
(`src/io/aio/tests/park.rs`), which builds the state directly and asserts on the
answer. No corpus file pins it end to end: the answer goes to a fiber that is
gone, so nothing in the program can observe it.
`tests/elle/io-stale-operation-ends.lisp` reaches the same state and asserts on
what a program CAN see, which is the operation ending.

### Ending an operation whose fiber is gone

Withholding the result is half the answer. The completion still has to arrive,
and an operation that parks arrives only when something outside this process
acts. A read waits for a peer that may never write, an accept for a connection
nobody makes. The fiber that would have received the result is gone, so nothing
in the program is left to make that event happen.

The backend therefore ends these operations itself.
`PendingTable::orphaned_to_stop` reports the in-flight ids whose asking fiber
has reached a terminal state, and every drain asks each of them to stop before
it waits. The ask goes through the stop pipe on the pool and through
`IORING_OP_ASYNC_CANCEL` on the ring, the two halves `io/cancel` also uses. The
operation completes with `-ECANCELED`, `take` reports it `Orphaned`, and the
entry is retired and answered as above.

Asking the fiber rather than its memory is what keeps an unwinding fiber out of
this. `fiber/abort` resumes a fiber to unwind, and that unwinding can suspend
and be resumed again (`docs/signals/primitives.md` § "Unwinding that suspends"),
so such a fiber is `:paused` and still has a result to come back for. It reaches
a terminal state when it is genuinely finished, and not before.

The ask is deliberately not a cancel. A cancel marks the id, and a marked id
falls silent; this one must answer, because the scheduler still holds the
pairing and lets go only on a completion. The table records each id it has
reported, so each worker is asked once — a drain runs on every loop tick, and
the completion takes a moment to come back.

Operations that end on their own are asked too, and the ask reaches nothing.
`Resolve` runs to the resolver's own end and `Task` runs until its closure
returns, so neither carries a stop pipe; their completions arrive as they always
would.

The runtime does not lean on a peer to deliver these. A peer that writes wakes
the parked worker, and the completion then arrives without the sweep having
asked for anything — but that needs a peer, and the peers these operations wait
on are exactly the ones that may never act. Nor does it lean on the close of the
descriptor, which is the platform's choice rather than a promise (§ "The stop
pipe").

Pinned by `an_operation_that_parks_ends_when_the_fiber_that_asked_is_gone`
(`src/io/aio/tests/park.rs`), which gives the operation no peer at all, and end
to end by `tests/elle/io-stale-operation-ends.lisp`.

### The stop pipe

An operation that can wait indefinitely carries a **stop pipe**, and that
is what lets it stop at once rather than when its peer happens to act.
`CompletionHub::bounds` opens one per submission and hands it to the worker
inside the operation's `Bounds`: the worker owns the read end for the
operation's lifetime and polls it alongside its own descriptor through
`OpBound`, while `CompletionHub::stop` writes one byte into the write end. The
operation then completes with `-ECANCELED`, which gives the worker thread back
like any other completion.

Polling a pipe is what keeps the descriptor intact. Shutting the socket
down would reach the worker too, and would break a port the caller still
holds; a signal would land on whichever thread the kernel chose.

Two conditions decide which operations carry one:

- **It can wait for something that may never happen.** The reads, the write,
  `Sleep`, `Accept`, `RecvFrom`, both connects, `PollFd`, `WatchRead`, the
  signal reads, `ProcessWait` and `Open` all wait on something outside this
  process — a peer, an event, a child, a fifo's other end. The write's peer is
  the one that reads: the full-write invariant makes the operation run to the
  end of its payload, and a payload larger than the send buffer only gets there
  as the peer takes what is already in it. A peer that stops reading fills the
  buffer and parks the write in `bound.wait(POLLOUT)`, so the write carries a
  pipe like the reads do. `Flush`, `SendTo` and `Shutdown` transfer what the
  process already handed over, so those three take no pipe (`Bounds::prompt`).
- **Stopping it must not park it instead.** `OpBound` takes the descriptor
  non-blocking for the operation's lifetime, so the syscall reports
  `EAGAIN` and hands the wait back to the poll where the stop is visible. A
  worker that calls the blocking syscall first is unreachable: closing the
  listener does not wake a thread already inside `accept(2)`.

Closing the descriptor is not the second ending it looks like. Whether a close
wakes a thread parked in `poll(2)` on that descriptor is the platform's choice:
macOS and the BSDs wake it, and Linux does not, because `poll` holds a reference
to the file it waits on. So an operation that can park carries a pipe on every
platform, and a close ends such an operation by writing to that pipe rather than
by being a close (§ "How a close wakes the operations it retires").

Two operations meet the first condition and cannot meet the second, because the
kernel reports no readiness for what they wait on: an `Open` of a fifo for
writing, and a `ProcessWait` on a child that has not exited. Both ask with a
non-blocking form — `O_NONBLOCK`, `WNOHANG` — and `pace_retry` waits between
asks with the stop pipe visible throughout. `connect_bounded` does the same for
an AF_UNIX peer whose backlog is full.

`Resolve` and `Task` meet neither, and say so with `Bounds::uninterruptible`.
`getaddrinfo(3)` runs to the resolver's own end and an opaque closure runs until
it returns; a cancel discards the result without giving the worker thread back
any sooner. Every use of that constructor names the call that behaves this way.

`ev/timeout` cancels on every call — the body's operation or the timer's,
whichever lost — so this path runs constantly rather than at the edges.

When the process is out of descriptors, `bounds` returns a `Bounds` with no
stop pipe and the operation runs uncancellable. It still bounds itself by the
caller's `:timeout`, which `OpBound` enforces with the same poll.

### Descriptor retirement

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

### How a close wakes the operations it retires

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

### How many operations run at once

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

### How a worker is reused

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

## One id, one operation

A completion carries an id and is resolved through the entry filed under it.
The arm that entry selects decides who owns the completion's payload:
`ProcessWait` reclaims a `Box<siginfo_t>`, `Connect` and `Open` take ownership
of a descriptor, the port arms write through a fiber's buffer. So a completion
that resolves to the wrong entry does not merely report the wrong thing — it
applies one operation's ownership rules to another operation's payload.

A pool worker therefore reports the `OpKind` it ran alongside the id, and
`pool_to_completion` asks `PendingOp::accepts` whether the entry could have
been filed by an operation of that kind. A "no" is the submission table and the
worker contradicting each other about one id. The completion is then withheld:
the entry is let go **unread** rather than retired, because retiring reclaims
the very payload in question, and the fiber is told what happened rather than
handed a result read through the wrong shape. The kinds are coarser than
`PendingOp` because they name what a worker can report having done —
`ev/poll-fd` and the `chan/wait-ready` park run the same operation, so both
answer to `OpKind::Poll`.

The stdin worker runs reads on a port and nothing else, so its completions
answer to `OpKind::Port`. io_uring has no such tag: a CQE's `user_data` is the
`u64` the SQE carried, returned by the kernel untouched.

Pinned by `a_completion_for_another_operation_is_withheld_from_the_entry_it_found`
and `a_pending_entry_accepts_only_the_kind_of_operation_that_filed_it`
(`src/io/aio/tests/submit.rs`).

## Assembling a read's answer

A finishing read owns two runs of bytes: the remainder a previous read on the
port left behind, and the bytes this read produced. `assemble_read`
(`src/io/completion/port.rs`) joins them into one `Vec` in stream order, the
op decides how much of that join answers the request, and whatever is past that
goes back to the port as its new remainder. One path serves `read`,
`read-line`, and `read-exact`, and serves them the same whether the stream
delivered bytes or ended.

The join is built outside the fiber's buffer because what a read reserves is
not a bound on what it answers with. A text `read-exact` counts grapheme
clusters and a cluster is any number of joined codepoints; a `read-line`
reserves 64 KiB and a line can be longer. So `read_result` writes the join back
into that buffer when it fits — keeping the caller's region and the zero-copy
`LBytes`→`LString` transmute — and builds the value on the requesting
instance's heap when it does not, exactly as `read-all` does. What it never
does is clamp: the bytes past a reservation are bytes the port has already
taken from the kernel, and nothing is left to read them again.

That is also why a pool worker's bytes stay in `pc.data` rather than being
staged into the fiber's buffer first, and why a remainder the submission could
not answer from stays in `fd_states` rather than being copied in ahead of the
read. `assemble_read` is the one place the two meet.

The submission answers from the remainder alone whenever it can, using
`frame::line_end` and `frame::exact_end` — the same two the completion cuts
with, so the submission and the completion cannot frame one stream two ways.
Answering there is not merely a saved syscall: a read submitted for bytes the
port is already holding would park until the peer sent more, and a peer that has
said everything never will.

Pinned by `tests/elle/port-text-framing.lisp` and
`tests/elle/port-longline.lisp`, and on the other backend by
`port_text_framing_threadpool` / `port_longline_threadpool`
(`tests/integration/elle_scripts.rs`).

## Buffer Drain Invariant

Buffered data is never lost on EOF or error. The backend drains buffered data before surfacing EOF or error status.

## Full-Write Invariant

`IoOp::Write` completes only when the whole payload has left for the fd. One
`write(2)` transfers at most what fits in the send buffer, so both backends
loop: the io_uring path resubmits the unwritten tail from the same pooled
buffer (`drain_cqes`, `PendingOp::Port.filled` counts the bytes already
accepted), and the thread-pool worker loops inside `PoolOp::Write`. The
completion reports `filled + result_code`, which equals the payload length.

A failure part-way through surfaces as an error, not as a short count — a
count smaller than the payload would read as success to a caller that trusts
the invariant. See `docs/io.md` and `tests/elle/port-shortwrite.lisp`.

## Operation timeouts

A request's `:timeout` bounds each kernel operation, not the whole call. Most
calls are one operation and the distinction does not arise. It arises for every
call that loops: `Write` until the payload is gone, `ReadExact` until its count,
`ReadAll` until EOF, `ReadLine` until a newline. For those, a peer that has
stalled must trip the deadline while one that is merely slow must not — a
per-call deadline would satisfy the first and break the second.

`Accept`, `RecvFrom` and both connects are single operations, and the bound
matters to them most: each waits on a peer that may never appear, so the
deadline is the only thing that ends them. A `connect` measures its deadline
across its retries, because one connect is one operation however many times the
kernel makes the worker ask.

Each backend carries the bound its own way:

| Backend | Mechanism | Expiry |
|---------|-----------|--------|
| io_uring | `push_resubmit` re-arms a `LinkTimeout` on every resubmission; `PendingOp::Port.timeout` carries the duration | `ECANCELED` |
| thread pool | `OpBound` holds the fd in non-blocking mode for the operation and waits for readiness in `poll(2)`, re-armed after every transfer | `ETIMEDOUT` |

`complete_port_op` maps both errnos to the `:timeout` error kind.

The bound holds for every kind of descriptor, which is why the pool worker owns
the wait rather than delegating it to the fd. `SO_RCVTIMEO`/`SO_SNDTIMEO` bound
a socket, and a pipe, a fifo and a tty all reject them — yet a reader that stops
reading fills a pipe exactly as it fills a socket, and the write that follows
parks in the kernel forever. `poll(2)` accepts every descriptor, so `OpBound`
bounds every descriptor. Non-blocking mode is what makes the wait sufficient: a
blocking syscall can park again after a poll reports the fd ready, while a
non-blocking one reports `EAGAIN` and hands the wait back to `OpBound`.

`O_NONBLOCK` lives on the open file description, so two operations on one
descriptor share it. `OpBound` counts them: the first operation sets the flag
and records what it found, the last one puts that back. Every read and write
loop treats `EAGAIN` as a readiness wait whether or not it asked for a timeout,
so an untimed operation that meets a descriptor another operation made
non-blocking waits rather than failing.

Pinned by `tests/elle/port-write-timeout.lisp` and
`tests/elle/port-read-timeout.lisp`, both run on each backend, each covering a
socket peer and a pipe peer. `tests/elle/net-wait-timeout.lisp` covers the
calls that wait for a peer, and `a_pool_connect_reports_its_own_deadline_as_a_timeout`
(`src/io/aio/tests/net.rs`) covers the connect, whose stall needs a listener
backlog an Elle script cannot set.

## The submission frame

Every operation the async backend issues passes through
`AsyncBackend::submit_op` (`src/io/aio/requests.rs`), which performs the four
steps each submission shares:

| Step | What it decides |
|---|---|
| `mint_id` | the id the operation carries to the kernel or to a worker |
| `buffer_pool.alloc(buf_bytes)` | the pinned bytes the kernel may write into, held until the completion arrives |
| `dispatch(&mut Dispatch)` | which platform runs the operation, and what its pending entry must remember |
| `pending.insert(id, ..)` | the entry the arriving completion is resolved by |

`Dispatch` bundles the id, the buffer handle, and the three backend fields a
dispatch reaches: `platform`, `hub`, `buffer_pool`. The `match platform` stays
at each call site — every operation calls a different `submit_uring_*` and
builds a different `PoolOp`, and the ring type does not exist off Linux, so a
shared helper would need a `#[cfg]`'d signature for no gain.
`Dispatch::poll_fd` is the one exception: `ev/poll-fd` and the
`chan/wait-ready` park wait on a bare descriptor the same way and differ only
in what they remember. Both watch that descriptor without changing it — it
belongs to whoever passed it in — so the pool worker takes `OpBound::watching`
rather than `OpBound::new`.

`Open` names no platform at all: it always goes to the pool. See invariant 14.

`dispatch` returns what the platform decided and the pending entry must record
— the pre-created socket fd for a connect, the `siginfo_t` allocation for a
process wait, `()` for the operations that decide nothing. `make_pending`
turns that plus the buffer handle into the `PendingOp`.

Two operations finish inside the submit call, so they take no buffer and file
no pending entry; both push a `Completion` straight onto the queue:

- `Spawn` — the child is started synchronously by `spawn_to_struct`.
- `ProcessWait` on a child whose exit status the handle's `ExitRecord` already
  holds.

A dispatch failure returns before any pending entry exists, and leaves the
buffer reserved: `submit_linked` can fail with the operation's SQE already
pushed onto the submission queue, so the kernel may still read that buffer on
the next `ring.submit()`.

## Backend Execution

### Subprocess Operations

**`SpawnRequest::spawn_to_struct()`** (in `request.rs`) — Spawns a subprocess using `std::process::Command`. Returns a struct with fields:
- `:pid` (int) — process ID
- `:stdin`, `:stdout`, `:stderr` (port or nil) — pipes created per `StdioDisposition`
- `:process` (external) — `ProcessHandle` for later wait operations

**`pipe_to_port()`** (in `request.rs`) — Converts a subprocess pipe (ChildStdin, ChildStdout, ChildStderr) to a Port Value.

**`AsyncBackend::submit_spawn()`** — Calls `spawn_to_struct()`. Spawn is an immediate completion (no CQE arrives); the result is pushed directly to the completions queue.

**`AsyncBackend::submit_process_wait()`** — Submits subprocess wait via `IORING_OP_WAITID` (Linux 6.7+), or on the thread pool. Fast path: if the handle's `ExitRecord` already holds a status, returns an immediate completion. Otherwise, allocates a `siginfo_t` buffer, submits the SQE, and stores the pending operation — with a clone of the record in both the `PoolOp` and the `PendingOp`.

**`child::process_wait()`** (in `src/io/threadpool/child.rs`) — the thread-pool half. `ExitRecord::reap` asks with `waitpid(pid, .., WNOHANG)` and returns either way, and `pace_retry` waits between asks with the stop pipe visible — starting at a millisecond and growing to fifty, so a child that exits at once is reported at once while a long-running one costs few wakeups. The blocking `waitpid` it replaces held the worker for the child's whole life, where neither `io/cancel` nor a deadline could reach it. Asking through the record is what keeps a reap this worker's cancellation discards — see § "A reap is never wasted". Pinned by `src/io/threadpool/tests/process.rs`.

**`submit_uring_process_wait()`** (in `src/io/uring.rs`) — Low-level io_uring submission for `IORING_OP_WAITID`. Requires Linux 6.7+; older kernels return `-EINVAL` (errno 22) in the CQE. The kernel fills the `siginfo_t` buffer on child exit; completion processing extracts the exit code from `si_code` and `si_status`.

## Invariants

1. `IoRequest` values are only created by stream and network primitives.
2. Backends are only created by `io/backend`.
3. The backend validates port direction and open status before I/O.
4. Stdio ports use `std::io::stdin()/stdout()/stderr()` handles directly.
5. Per-fd state is keyed by `PortKey` (Stdin/Stdout/Stderr/Fd(raw_fd)).
6. Buffer drain invariant: buffered data is never lost on EOF or error.
7. Buffers passed to io_uring must not move while the kernel holds them.
8. stdin reads in async mode go through a dedicated OS thread, not io_uring.
   That worker shares the single `CompletionHub` channel: its completions are
   `RawCompletion::Stdin` items like any other worker's. On the pool-only
   platform the scheduler blocks on one `recv()` of the hub (register-before-
   sleep — no source can be missed). On the io_uring platform the scheduler
   blocks on one `io_uring_enter`; hub work that posts no ring CQE (stdin /
   getaddrinfo / `Task`) wakes that single wait through a standing
   `POLL_ADD(eventfd, POLLIN)` — the eventfd bridge. A worker raises the eventfd
   (`publish_completion`) *after* publishing to the channel, so the wake can
   never precede the item; the wait clears the eventfd and re-arms the one-shot
   poll. One blocking primitive per platform, no wakeup-rescue caps: a genuinely
   lost wakeup hangs rather than being downgraded to a bounded stall.
9. `io/submit`, `io/reap`, `io/wait`, `io/cancel` only work with async backends.
10. Network operations are yielding (`SIG_IO`). Synchronous network setup (tcp/listen, udp/bind, unix/listen) does not yield.
11. **Dispatch-before-port-guard:** `Spawn` and `ProcessWait` must be dispatched before the `as_external::<Port>()` guard. `Spawn` has `Value::NIL` as its port field; `ProcessWait` has a `ProcessHandle` in the port field (not a `Port`).
12. **ProcessWait siginfo lifetime:** The `siginfo_t` buffer in `PendingOp::ProcessWait` is heap-allocated via `Box::into_raw` and must remain valid until the CQE arrives. Completion processing reclaims it via `Box::from_raw`, and so does `PendingOp::retire` — which reads the exit status out of it first, because a retired wait may be the one that reaped the child (§ "A reap is never wasted"). The fast path (a status already on the handle) never inserts a `PendingOp::ProcessWait`, so the buffer is only allocated for truly pending operations.
13. **IORING_OP_WAITID requirement:** Linux 6.7+; older kernels return `-EINVAL` in the CQE. The thread-pool backend reaps the child itself, asking with `WNOHANG` and pacing the asks under the operation's bound.
14. **Open runs on the thread pool, on every platform.** An `open(2)` of a fifo waits for the other end, and a wait is only answerable where the worker holds it: `IORING_OP_OPENAT` blocks an io-wq thread that a linked timeout marks cancelled but cannot retract. One implementation is also one answer — the fifo behaviour in `docs/io.md` is the same whichever platform is underneath. Pinned by `src/io/threadpool/tests/openfile.rs`.
15. **Seek/Tell are immediate completions.** `IoOp::Seek` and `IoOp::Tell` are never submitted to io_uring or the thread pool. They call `libc::lseek(2)` synchronously in the backend's submit/execute path and return an immediate completion. `PoolOp` has no `Seek` or `Tell` variant.
16. **Task dispatch:** `IoOp::Task` is dispatched before the port guard (it is portless). There is no io_uring equivalent for an arbitrary closure, so a `Task` always runs on the thread pool (feeding the `CompletionHub`) on every platform. The `TaskFn` closure is taken exactly once via `RefCell<Option<...>>`; double-take returns an error.
17. **One submission, one pending entry, one id.** `submit_op` files the
    `PendingOp` under the same id it dispatched the operation with, so an
    arriving completion always finds its entry. A completion whose entry is
    missing is discarded, and the fiber waiting on it never wakes — a hang,
    not an error. Pinned by `src/io/aio/tests/submit.rs`.
