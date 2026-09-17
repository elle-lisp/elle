# I/O Module

<!-- audited: 2026-09-16 -->

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
| `threadpool/pool.rs` | `WorkerPool`, `Crew` and `Job` — the parked workers' handoffs, and the choice between handing a job to one of them and starting a thread. See [descriptors and workers](../../docs/impl/io-descriptor.md). |
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

**POSIX signal reception:** `SigNext` — portless; the `SignalReceiver` external lives in `IoRequest.port`. Reads from the signalfd (Linux) / kqueue fd (macOS) opened by `os/sig-watch`. Completion is an array of structs `{:signal :sigterm :sender-pid n :sender-uid n :code n :count n}`. See [posix signals](../../docs/posix-signals.md) for the mask policy that backs this op.

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

- `Port { op, port_key, port, descriptor, buffer_handle, listener_kind, filled, timeout }` — operation on an existing port (stream I/O, accept, datagram, shutdown). `descriptor` is this operation's share of the number it names — see [descriptors and workers](../../docs/impl/io-descriptor.md). `listener_kind` is `Some(PortKind)` for Accept only.
- `Connect { addr, buffer_handle, connect_fd, port }` — creates a new port on completion. `connect_fd` starts as `Some(fd)` for io_uring (pre-created socket) or `None` for thread pool (set on completion).
- `Open { path, buffer_handle, port }` — creates a new port on completion; `path` is kept for the error message.
- `Sleep { buffer_handle }` — portless timer.
- `ProcessWait { buffer_handle, handle_val, siginfo, exit }` — waiting for subprocess exit via IORING_OP_WAITID. `siginfo` is a heap-allocated `siginfo_t` filled by the kernel; released in completion processing. Null on the thread-pool path, where the worker reports the exit code itself. `exit` is a clone of the handle's `ExitRecord`, so the entry can keep a reaped status without dereferencing `handle_val` — see § "A reap is never wasted".
- `Task { buffer_handle }` — background task running on thread pool.
- `Resolve { buffer_handle }` — getaddrinfo(3) on the thread pool.
- `WatchNext { watcher, buffer_handle }` / `SigNext { receiver, buffer_handle }` — a read on the inotify / signalfd descriptor the external owns. Both are operands, so the entry's hold keeps the external — and therefore the descriptor it owns — for the read's lifetime; see [descriptors and workers](../../docs/impl/io-descriptor.md).
- `PollFd { buffer_handle }` — readiness wait on a bare descriptor.
- `ChanSelectPark { buffer_handle, guard }` — readiness wait on a `chan/wait-ready` wake fd. The guard owns the fd(s) and the wake-list registrations, so dropping this entry deregisters exactly once.

### PoolOp / PoolCompletion / RawCompletion / CompletionHub

Typed thread-pool submission and completion:

- `PoolOp` — one variant per operation the pool runs. Each carries exactly the data that operation needs (fd, buffers, addresses, or closures) and nothing about waiting: a typed submission.
- `Bounds` — how long an operation may wait and how `io/cancel` ends it, passed alongside the `PoolOp` to every `CompletionHub::submit`. Three constructors, and a submission must pick one: `CompletionHub::bounds(id, timeout)` pairs the caller's deadline with a fresh stop pipe, `Bounds::prompt()` says the syscalls wait on nothing outside this process, and `Bounds::uninterruptible()` says the syscall cannot be stopped once entered. Because the bound is an argument rather than a field, a variant cannot forget it. The `Bounds` own the stop pipe's read end and close it with themselves, so a submission no worker runs — a refused `Builder::spawn`, a path the kernel rejects — disposes of the pipe by being dropped.
- `OpBound` — what a worker runs under: it holds the descriptor non-blocking for the operation's lifetime and turns the declared `Bounds` into waits. `OpBound::new(fd, ..)` for an operation that reads or writes `fd`, `OpBound::watching(fd, ..)` for one that only polls a descriptor somebody else owns, `OpBound::detached(..)` for one with no descriptor at all.
- `PoolCompletion { id, kind, result_code, data }` — typed completion from a thread-pool worker. `kind` is the `OpKind` the worker ran, checked against the entry the id resolves through — see [an operation in flight](../../docs/impl/io-inflight.md).
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
  `pool` is the crew that runs the operations — see [descriptors and workers](../../docs/impl/io-descriptor.md).

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
the mechanism and [an operation in flight](../../docs/impl/io-inflight.md) for the cancellation half.

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
waiter and by a killer. [subprocesses](../../docs/subprocess.md) carries the argument and the answers the primitive gives, and
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


The rest of the cancellation story is two documents of its own:

- [an operation in flight](../../docs/impl/io-inflight.md) — the values a
  submitted operation holds, when it lets them go, how an operation whose fiber
  is gone is ended, and how a completion's answer is assembled.
- [descriptors and workers](../../docs/impl/io-descriptor.md) — what a close
  retires, how it wakes the operations holding the descriptor, and how the pool
  reuses a worker.


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
the invariant. See [io](../../docs/io.md) and `tests/elle/port-shortwrite.lisp`.

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

- `Spawn` — the child is started synchronously by `spawn_to_subprocess`.
- `ProcessWait` on a child whose exit status the handle's `ExitRecord` already
  holds.

A dispatch failure returns before any pending entry exists, and leaves the
buffer reserved: `submit_linked` can fail with the operation's SQE already
pushed onto the submission queue, so the kernel may still read that buffer on
the next `ring.submit()`.

## Backend Execution

### Subprocess Operations

**`SpawnRequest::spawn_to_subprocess()`** (in `request.rs`) — Spawns a subprocess using `std::process::Command`. Returns one external, type name `subprocess`, wrapping a `ProcessHandle` that carries:
- the pid
- the `stdin`, `stdout` and `stderr` port `Value`s (or `Value::NIL`), created per `StdioDisposition`
- the `ExitRecord` every later wait and kill reads

The ports and the handle are minted through one `Alloc` over the requesting instance's heap, so they share its region and hold no cross-heap reference. There is no wrapper struct: the external is what `subprocess/wait` and `subprocess/kill` take.

**`pipe_to_port()`** (in `request.rs`) — Converts a subprocess pipe (ChildStdin, ChildStdout, ChildStderr) to a Port Value.

**`AsyncBackend::submit_spawn()`** — Calls `spawn_to_subprocess()`. Spawn is an immediate completion (no CQE arrives); the result is pushed directly to the completions queue.

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
14. **Open runs on the thread pool, on every platform.** An `open(2)` of a fifo waits for the other end, and a wait is only answerable where the worker holds it: `IORING_OP_OPENAT` blocks an io-wq thread that a linked timeout marks cancelled but cannot retract. One implementation is also one answer — the fifo behavior in [io](../../docs/io.md) is the same whichever platform is underneath. Pinned by `src/io/threadpool/tests/openfile.rs`.
15. **Seek/Tell are immediate completions.** `IoOp::Seek` and `IoOp::Tell` are never submitted to io_uring or the thread pool. They call `libc::lseek(2)` synchronously in the backend's submit/execute path and return an immediate completion. `PoolOp` has no `Seek` or `Tell` variant.
16. **Task dispatch:** `IoOp::Task` is dispatched before the port guard (it is portless). There is no io_uring equivalent for an arbitrary closure, so a `Task` always runs on the thread pool (feeding the `CompletionHub`) on every platform. The `TaskFn` closure is taken exactly once via `RefCell<Option<...>>`; double-take returns an error.
17. **One submission, one pending entry, one id.** `submit_op` files the
    `PendingOp` under the same id it dispatched the operation with, so an
    arriving completion always finds its entry. A completion whose entry is
    missing is discarded, and the fiber waiting on it never wakes — a hang,
    not an error. Pinned by `src/io/aio/tests/submit.rs`.
