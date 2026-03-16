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
| `pending.rs` | `PendingOp` enum — in-flight async operation tracking (4 variants) |
| `aio.rs` | `AsyncBackend` — async I/O with io_uring (Linux) or thread-pool fallback |
| `request.rs` | `IoRequest` and `IoOp` types — typed I/O request descriptors |
| `completion.rs` | `process_raw_completion` — converts raw CQE/thread results to `Completion` |
| `sockaddr.rs` | Sockaddr construction, formatting, parsing — single source of truth |
| `threadpool.rs` | `ThreadPoolBackend`, `PoolOp`, `PoolCompletion` — typed thread-pool I/O |
| `uring.rs` | io_uring SQE submission and CQE processing (Linux only) |
| `backend/` | `SyncBackend` — synchronous I/O execution with per-fd buffering |

## Data Flow

Sync path:
```
Stream primitive → (SIG_IO, IoRequest) → Scheduler → io/execute → SyncBackend → OS
```

Async path:
```
Stream primitive → (SIG_IO, IoRequest) → Scheduler → io/submit → AsyncBackend → OS (async)
                                                    ← io/wait  ← completions ← OS
```

## Key Types

### IoOp

Enum of I/O operations (13 variants):

**Stream operations:** `ReadLine`, `Read { count }`, `ReadAll`, `Write { data }`, `Flush`

**Network operations:** `Accept`, `Connect { addr }`, `SendTo { addr, port_num, data }`, `RecvFrom { count }`, `Shutdown { how }`

**Timer:** `Sleep { duration }`

**Subprocess operations:** `Spawn { program, args, env, cwd, stdin, stdout, stderr }`, `ProcessWait`

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

Enum tracking in-flight async operations (4 variants):

- `Port { op, port_key, port, buffer_handle, listener_kind }` — operation on an existing port (stream I/O, accept, datagram, shutdown). `listener_kind` is `Some(PortKind)` for Accept only.
- `Connect { addr, buffer_handle, connect_fd }` — creates a new port on completion. `connect_fd` starts as `Some(fd)` for io_uring (pre-created socket) or `None` for thread pool (set on completion).
- `Sleep { buffer_handle }` — portless timer.
- `ProcessWait { buffer_handle, handle_val, siginfo }` — waiting for subprocess exit via IORING_OP_WAITID. `siginfo` is a heap-allocated `siginfo_t` filled by the kernel; released in completion processing.

### PoolOp / PoolCompletion

Typed thread-pool submission and completion:

- `PoolOp` — enum with 10 variants matching the operations. Each variant carries exactly the data that operation needs (fd, buffers, addresses). Replaces the old `(fd, op_kind: u8, data: Vec<u8>, size: usize)` untyped submission.
- `PoolCompletion { id, result_code, data }` — typed completion struct. Replaces the old `(u64, i32, Vec<u8>)` tuple.

### ConnectAddr

Enum: `Tcp { addr, port }` or `Unix { path }`.

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
| `io/execute` | errors | Execute an I/O request on a backend (blocking) |
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

## Backend Execution (Chunk 2)

### Subprocess Operations

Both `SyncBackend` and `AsyncBackend` implement subprocess spawn and wait:

**`SyncBackend::execute_spawn()`** — Spawns a subprocess synchronously using `std::process::Command`. Returns a struct with fields:
- `:pid` (int) — process ID
- `:stdin`, `:stdout`, `:stderr` (port or nil) — pipes created per `StdioDisposition`
- `:process` (external) — `ProcessHandle` for later wait operations

**`SyncBackend::execute_process_wait()`** — Waits for subprocess exit using `child.wait()`. Returns exit code (int). Caches the exit code in `ProcessHandle` for idempotent re-waits.

**`AsyncBackend::submit_spawn()`** — Spawns a subprocess asynchronously. Spawn is an immediate completion (no CQE arrives); the result is pushed directly to the completions queue.

**`AsyncBackend::submit_process_wait()`** — Submits subprocess wait via `IORING_OP_WAITID` (Linux 6.7+). Fast path: if the process has already exited (cached in `ProcessHandle`), returns immediate completion. Otherwise, allocates a `siginfo_t` buffer, submits the SQE, and stores the pending operation.

**`submit_uring_process_wait()`** (in `src/io/uring.rs`) — Low-level io_uring submission for `IORING_OP_WAITID`. Requires Linux 6.7+; older kernels return `-EINVAL` (errno 22) in the CQE. The kernel fills the `siginfo_t` buffer on child exit; completion processing extracts the exit code from `si_code` and `si_status`.

**`pipe_to_port()`** (in `src/io/backend/mod.rs`) — Free function converting a subprocess pipe (ChildStdin, ChildStdout, ChildStderr) to a Port Value. Used by both sync and async backends.

### Encoding Error Handling

`SyncBackend::bytes_to_value()` now returns `SIG_ERROR` with kind `"encoding-error"` when `Encoding::Text` encounters invalid UTF-8, instead of silently replacing invalid bytes with the replacement character. This prevents data corruption and makes encoding errors visible to the caller.

## Invariants

1. `IoRequest` values are only created by stream and network primitives.
2. Backends are only created by `io/backend`.
3. The backend validates port direction and open status before I/O.
4. Stdio ports use `std::io::stdin()/stdout()/stderr()` handles directly.
5. Per-fd state is keyed by `PortKey` (Stdin/Stdout/Stderr/Fd(raw_fd)).
6. Buffer drain invariant: buffered data is never lost on EOF or error.
7. Buffers passed to io_uring must not move while the kernel holds them.
8. stdin reads in async mode go through a dedicated OS thread, not io_uring.
9. `io/submit`, `io/reap`, `io/wait`, `io/cancel` only work with async backends.
10. Network operations are yielding (`SIG_IO`). Synchronous network setup (tcp/listen, udp/bind, unix/listen) does not yield.
11. **Dispatch-before-port-guard:** In both sync and async backends, `Spawn` and `ProcessWait` must be dispatched before the `as_external::<Port>()` guard. `Spawn` has `Value::NIL` as its port field; `ProcessWait` has a `ProcessHandle` in the port field (not a `Port`). This ensures subprocess ops are handled before the code attempts to extract a Port from the request.
12. **Encoding errors:** `Encoding::Text` read errors now return `SIG_ERROR` with kind `"encoding-error"` instead of silently replacing invalid UTF-8 with the replacement character.
13. **ProcessWait siginfo lifetime:** The `siginfo_t` buffer in `PendingOp::ProcessWait` is heap-allocated via `Box::into_raw` and must remain valid until the CQE arrives. Completion processing reclaims it via `Box::from_raw`. The fast path (already exited) never inserts a `PendingOp::ProcessWait`, so the buffer is only allocated for truly pending operations.
14. **IORING_OP_WAITID requirement:** Linux 6.7+. Thread-pool backend returns error for `ProcessWait`. Older kernels return `-EINVAL` in the CQE.
