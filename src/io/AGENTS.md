# I/O Module

## Purpose

The `io` module contains I/O request types and backends for Elle's
scheduler-based I/O system. Stream primitives build `IoRequest` values
and yield them via `SIG_IO`. The scheduler catches `SIG_IO` and dispatches
to a backend for execution.

## Modules

| Module | Responsibility |
|--------|----------------|
| `types.rs` | Shared types: `PortKey`, `FdState`, `FdStatus` — used by both backends |
| `pool.rs` | `BufferPool`, `BufferHandle` — pinned buffer management for async I/O |
| `pending.rs` | `PendingOp` enum — in-flight async operation tracking (5 variants) |
| `aio.rs` | `AsyncBackend` — async I/O with io_uring (Linux) or thread-pool fallback |
| `request.rs` | `IoRequest` and `IoOp` types — typed I/O request descriptors |
| `completion.rs` | `process_raw_completion` — converts raw CQE/thread results to `Completion` |
| `sigfd.rs` | `SignalReceiver` — POSIX signalfd (Linux) or kqueue+EVFILT_SIGNAL (macOS) external for `os/sig-watch`; also the worker-thread mask helper `mask_all_signals_on_this_thread` |
| `sigmap.rs` | Shared keyword↔signum mapping; `resolve(value, ctx)` parses a `:sigterm`/integer Value to libc signum |
| `sockaddr.rs` | Sockaddr construction, formatting, parsing — single source of truth |
| `threadpool.rs` | `CompletionHub` (the one shared completion channel), `RawCompletion`, `PoolOp`, `PoolCompletion`, `StdinThread` — typed thread-pool I/O. Every spawned worker calls `crate::io::sigfd::mask_all_signals_on_this_thread()` first so the kernel never selects it as a POSIX-signal delivery target. |
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

Enum tracking in-flight async operations (5 variants):

- `Port { op, port_key, port, buffer_handle, listener_kind }` — operation on an existing port (stream I/O, accept, datagram, shutdown). `listener_kind` is `Some(PortKind)` for Accept only.
- `Connect { addr, buffer_handle, connect_fd }` — creates a new port on completion. `connect_fd` starts as `Some(fd)` for io_uring (pre-created socket) or `None` for thread pool (set on completion).
- `Sleep { buffer_handle }` — portless timer.
- `ProcessWait { buffer_handle, handle_val, siginfo }` — waiting for subprocess exit via IORING_OP_WAITID. `siginfo` is a heap-allocated `siginfo_t` filled by the kernel; released in completion processing.
- `Task { buffer_handle }` — background task running on thread pool.

### PoolOp / PoolCompletion / RawCompletion / CompletionHub

Typed thread-pool submission and completion:

- `PoolOp` — enum with 11 variants matching the operations. Each variant carries exactly the data that operation needs (fd, buffers, addresses, or closures): a typed submission.
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

**Thread-pool fallback:** `SO_RCVTIMEO`/`SO_SNDTIMEO` on the fd, or `poll()` with timeout.

## I/O Cancellation

`io/cancel` submits `IORING_OP_ASYNC_CANCEL` on io_uring, or removes the pending entry on thread pool. The cancel SQE's CQE uses the high-bit tag (same as timeout CQEs) and is skipped by `drain_cqes`. The cancelled operation generates a CQE with `result = -ECANCELED`.

Used by `do-shutdown` in stdlib to cancel pending I/O before aborting/cancelling fibers.

## Buffer Drain Invariant

Buffered data is never lost on EOF or error. The backend drains buffered data before surfacing EOF or error status.

## Backend Execution

### Subprocess Operations

**`SpawnRequest::spawn_to_struct()`** (in `request.rs`) — Spawns a subprocess using `std::process::Command`. Returns a struct with fields:
- `:pid` (int) — process ID
- `:stdin`, `:stdout`, `:stderr` (port or nil) — pipes created per `StdioDisposition`
- `:process` (external) — `ProcessHandle` for later wait operations

**`pipe_to_port()`** (in `request.rs`) — Converts a subprocess pipe (ChildStdin, ChildStdout, ChildStderr) to a Port Value.

**`AsyncBackend::submit_spawn()`** — Calls `spawn_to_struct()`. Spawn is an immediate completion (no CQE arrives); the result is pushed directly to the completions queue.

**`AsyncBackend::submit_process_wait()`** — Submits subprocess wait via `IORING_OP_WAITID` (Linux 6.7+). Fast path: if the process has already exited (cached in `ProcessHandle`), returns immediate completion. Otherwise, allocates a `siginfo_t` buffer, submits the SQE, and stores the pending operation.

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
13. **ProcessWait siginfo lifetime:** The `siginfo_t` buffer in `PendingOp::ProcessWait` is heap-allocated via `Box::into_raw` and must remain valid until the CQE arrives. Completion processing reclaims it via `Box::from_raw`. The fast path (already exited) never inserts a `PendingOp::ProcessWait`, so the buffer is only allocated for truly pending operations.
14. **IORING_OP_WAITID requirement:** Linux 6.7+. Thread-pool backend returns error for `ProcessWait`. Older kernels return `-EINVAL` in the CQE.
15. **Seek/Tell are immediate completions.** `IoOp::Seek` and `IoOp::Tell` are never submitted to io_uring or the thread pool. They call `libc::lseek(2)` synchronously in the backend's submit/execute path and return an immediate completion. `PoolOp` has no `Seek` or `Tell` variant.
16. **Task dispatch:** `IoOp::Task` is dispatched before the port guard (it is portless). There is no io_uring equivalent for an arbitrary closure, so a `Task` always runs on the thread pool (feeding the `CompletionHub`) on every platform. The `TaskFn` closure is taken exactly once via `RefCell<Option<...>>`; double-take returns an error.
