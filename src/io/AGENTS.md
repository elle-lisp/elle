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
| `pending.rs` | `PendingOp` enum — in-flight async operation tracking, one variant per operation shape |
| `aio.rs` | `AsyncBackend` — async I/O with io_uring (Linux) or thread-pool fallback |
| `request.rs` | `IoRequest` and `IoOp` types — typed I/O request descriptors |
| `completion.rs` | `process_raw_completion` — converts raw CQE/thread results to `Completion` |
| `sigfd.rs` | `SignalReceiver` — POSIX signalfd (Linux) or kqueue+EVFILT_SIGNAL (macOS) external for `os/sig-watch`; also the worker-thread mask helper `mask_all_signals_on_this_thread` |
| `sigmap.rs` | Shared keyword↔signum mapping; `resolve(value, ctx)` parses a `:sigterm`/integer Value to libc signum |
| `sockaddr.rs` | Sockaddr construction, formatting, parsing — single source of truth |
| `threadpool.rs` | `CompletionHub` (the one shared completion channel), `RawCompletion`, `PoolOp`, `PoolCompletion`, `StdinThread` — typed thread-pool I/O. Every spawned worker calls `crate::io::sigfd::mask_all_signals_on_this_thread()` first so the kernel never selects it as a POSIX-signal delivery target. |
| `threadpool/opbound.rs` | `Bounds` and `OpBound` — the declared and the live half of one operation's bound — plus `Wake`, the stop pipe, `take_when_ready` and `pace_retry`. |
| `threadpool/submitop.rs` | `CompletionHub::submit`: spawn the worker, run the operation, publish the result. The `match` there names each operation's runner and the descriptor its bound watches. |
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
- `inner: RefCell<ProcessState>` — lifecycle state (Running or Exited with cached exit code)

Methods:
- `new(pid, child) → ProcessHandle` — create from spawned child process
- `pid() → u32` — get process ID
- `Drop` impl — calls `try_wait()` on the child to reap zombies

### PendingOp

What one in-flight operation's completion needs, one variant per operation
shape. Every variant carries a `BufferHandle`; the rest is what that operation
alone must remember:

- `Port { op, port_key, port, buffer_handle, listener_kind, filled, timeout }` — operation on an existing port (stream I/O, accept, datagram, shutdown). `listener_kind` is `Some(PortKind)` for Accept only.
- `Connect { addr, buffer_handle, connect_fd, port }` — creates a new port on completion. `connect_fd` starts as `Some(fd)` for io_uring (pre-created socket) or `None` for thread pool (set on completion).
- `Open { path, buffer_handle, port }` — creates a new port on completion; `path` is kept for the error message.
- `Sleep { buffer_handle }` — portless timer.
- `ProcessWait { buffer_handle, handle_val, siginfo }` — waiting for subprocess exit via IORING_OP_WAITID. `siginfo` is a heap-allocated `siginfo_t` filled by the kernel; released in completion processing. Null on the thread-pool path, where the worker reports the exit code itself.
- `Task { buffer_handle }` — background task running on thread pool.
- `Resolve { buffer_handle }` — getaddrinfo(3) on the thread pool.
- `WatchNext { watcher, buffer_handle }` / `SigNext { receiver, buffer_handle }` — a read on the inotify / signalfd descriptor. The external is held so it outlives the read.
- `PollFd { buffer_handle }` — readiness wait on a bare descriptor.
- `ChanSelectPark { buffer_handle, guard }` — readiness wait on a `chan/wait-ready` wake fd. The guard owns the fd(s) and the wake-list registrations, so dropping this entry deregisters exactly once.

### PoolOp / PoolCompletion / RawCompletion / CompletionHub

Typed thread-pool submission and completion:

- `PoolOp` — one variant per operation the pool runs. Each carries exactly the data that operation needs (fd, buffers, addresses, or closures) and nothing about waiting: a typed submission.
- `Bounds` — how long an operation may wait and how `io/cancel` ends it, passed alongside the `PoolOp` to every `CompletionHub::submit`. Three constructors, and a submission must pick one: `CompletionHub::bounds(id, timeout)` pairs the caller's deadline with a fresh stop pipe, `Bounds::prompt()` says the syscalls wait on nothing outside this process, and `Bounds::uninterruptible()` says the syscall cannot be stopped once entered. Because the bound is an argument rather than a field, a variant cannot forget it. The `Bounds` own the stop pipe's read end and close it with themselves, so a submission no worker runs — a refused `Builder::spawn`, a path the kernel rejects — disposes of the pipe by being dropped.
- `OpBound` — what a worker runs under: it holds the descriptor non-blocking for the operation's lifetime and turns the declared `Bounds` into waits. `OpBound::new(fd, ..)` for an operation that reads or writes `fd`, `OpBound::watching(fd, ..)` for one that only polls a descriptor somebody else owns, `OpBound::detached(..)` for one with no descriptor at all.
- `PoolCompletion { id, result_code, data }` — typed completion from a thread-pool worker.
- `RawCompletion` — `Pool(PoolCompletion)` | `Stdin(StdinCompletion)`. The single
  shape every background worker ships through the hub. A worker cannot build a
  cooked `Completion` (the cook fns need main-thread `pending`/`fd_states`/
  `buffer_pool`/`origin_heap`), so it sends its raw result; the receiver matches
  once and dispatches to `pool_to_completion` / `stdin_to_completion`.
- `CompletionHub { sender, receiver, in_flight, eventfd }` — the **one** completion
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
  edge.

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

`io/cancel` submits `IORING_OP_ASYNC_CANCEL` on io_uring. The cancel SQE's CQE uses the high-bit tag (same as timeout CQEs) and is skipped by `drain_cqes`. The cancelled operation generates a CQE with `result = -ECANCELED`.

Used by `do-shutdown` in stdlib to cancel pending I/O before aborting/cancelling fibers, and by `ev/timeout`, which cancels whichever of the body and the timer lost.

### What cancellation promises on the thread pool

A pool operation runs on its own worker thread, which holds a plain
descriptor number and calls a syscall on it. No other thread can retract a
syscall already running, so a cancel asks rather than interrupts: the stop
pipe below is how it asks, and two things hold however the worker answers:

- **The submission is accounted for.** `cancel` marks the id in
  `cancelled` and leaves the `pending` entry in place. The worker's
  completion still arrives, still finds its entry, and still decrements
  `in_flight`; the cooked result is then thrown away instead of being
  handed to a fiber. Removing the entry instead would strand the
  submission: the worker's thread would never be accounted for again,
  and cancellation is a path `ev/timeout` takes on every call.
- **The descriptor outlives the operation.** See "Descriptor retirement"
  below.

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

- **It can wait for something that may never happen.** The reads, `Sleep`,
  `Accept`, `RecvFrom`, both connects, `PollFd`, `WatchRead`, the signal reads,
  `ProcessWait` and `Open` all wait on something outside this process — a peer,
  an event, a child, a fifo's other end. A `Write` runs to the end of its
  payload as the full-write invariant promises, and `Flush`, `SendTo` and
  `Shutdown` transfer what the process already handed over, so those four take
  no pipe (`Bounds::prompt`).
- **Stopping it must not park it instead.** `OpBound` takes the descriptor
  non-blocking for the operation's lifetime, so the syscall reports
  `EAGAIN` and hands the wait back to the poll where the stop is visible. A
  worker that calls the blocking syscall first is unreachable: closing the
  listener does not wake a thread already inside `accept(2)`.

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

`port/close` therefore hands the port's `OwnedFd` to the backend rather
than dropping it whenever `pending` still holds an operation on that key.
The port reports closed immediately, so Elle's semantics do not change;
the descriptor itself is closed by `retire_fd_if_drained` once the last
operation naming it has completed. `fd_states` for the key is dropped at
the same moment, so per-fd buffering never spans two ports either.

Pinned by `tests/elle/io-cancel-releases.lisp`.

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

The OS decides. A pool operation is one `std::thread::Builder::spawn`, so
the ceiling is `RLIMIT_NPROC`, `kernel.threads-max` and the memory for the
stacks — limits the operator set, on a machine the runtime cannot survey.
`Builder::spawn` rather than `thread::spawn` is what makes deferring to
them possible: `thread::spawn` panics when the OS refuses, while a
`Builder` refusal becomes the error `io/submit` returns and the calling
fiber can handle.

`io/workers` reports how many operations are submitted and not yet reaped
— the threads out right now — and `ev/report` carries it as `:workers`.
That is a measurement, not a budget: nothing consults it to decide whether
a submission may proceed.

io_uring has no equivalent count. Its operations run in the kernel, so
`workers()` is zero there and the only limit is the 256-entry submission
queue, which drains as it is submitted.

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
- `ProcessWait` on a child whose exit code is already cached in its
  `ProcessHandle`.

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

**`AsyncBackend::submit_process_wait()`** — Submits subprocess wait via `IORING_OP_WAITID` (Linux 6.7+), or on the thread pool. Fast path: if the process has already exited (cached in `ProcessHandle`), returns immediate completion. Otherwise, allocates a `siginfo_t` buffer, submits the SQE, and stores the pending operation.

**`child::process_wait()`** (in `src/io/threadpool/child.rs`) — the thread-pool half. `waitpid(pid, .., WNOHANG)` asks whether the child has exited and returns either way, and `pace_retry` waits between asks with the stop pipe visible — starting at a millisecond and growing to fifty, so a child that exits at once is reported at once while a long-running one costs few wakeups. The blocking `waitpid` it replaces held the worker for the child's whole life, where neither `io/cancel` nor a deadline could reach it. Pinned by `src/io/threadpool/tests/process.rs`.

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
12. **ProcessWait siginfo lifetime:** The `siginfo_t` buffer in `PendingOp::ProcessWait` is heap-allocated via `Box::into_raw` and must remain valid until the CQE arrives. Completion processing reclaims it via `Box::from_raw`. The fast path (already exited) never inserts a `PendingOp::ProcessWait`, so the buffer is only allocated for truly pending operations.
13. **IORING_OP_WAITID requirement:** Linux 6.7+; older kernels return `-EINVAL` in the CQE. The thread-pool backend reaps the child itself, asking with `WNOHANG` and pacing the asks under the operation's bound.
14. **Open runs on the thread pool, on every platform.** An `open(2)` of a fifo waits for the other end, and a wait is only answerable where the worker holds it: `IORING_OP_OPENAT` blocks an io-wq thread that a linked timeout marks cancelled but cannot retract. One implementation is also one answer — the fifo behaviour in `docs/io.md` is the same whichever platform is underneath. Pinned by `src/io/threadpool/tests/openfile.rs`.
15. **Seek/Tell are immediate completions.** `IoOp::Seek` and `IoOp::Tell` are never submitted to io_uring or the thread pool. They call `libc::lseek(2)` synchronously in the backend's submit/execute path and return an immediate completion. `PoolOp` has no `Seek` or `Tell` variant.
16. **Task dispatch:** `IoOp::Task` is dispatched before the port guard (it is portless). There is no io_uring equivalent for an arbitrary closure, so a `Task` always runs on the thread pool (feeding the `CompletionHub`) on every platform. The `TaskFn` closure is taken exactly once via `RefCell<Option<...>>`; double-take returns an error.
17. **One submission, one pending entry, one id.** `submit_op` files the
    `PendingOp` under the same id it dispatched the operation with, so an
    arriving completion always finds its entry. A completion whose entry is
    missing is discarded, and the fiber waiting on it never wakes — a hang,
    not an error. Pinned by `src/io/aio/tests/submit.rs`.
