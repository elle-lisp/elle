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
| `aio.rs` | `AsyncBackend` — async I/O with io_uring (Linux) or thread-pool fallback |
| `request.rs` | `IoRequest` and `IoOp` types — typed I/O request descriptors |
| `backend.rs` | `SyncBackend` — synchronous I/O execution with per-fd buffering |

## Data Flow

Sync path:
```
Stream primitive → (SIG_IO, IoRequest) → Scheduler → io/execute → SyncBackend → OS
```

1. Stream primitives (`stream/read-line`, `stream/read`, `stream/read-all`,
   `stream/write`, `stream/flush`) build an `IoRequest` and return
   `(SIG_IO, request)`, suspending the fiber.
2. The scheduler catches `SIG_IO`, extracts the `IoRequest` from the
   fiber's signal value.
3. The scheduler calls `io/execute` with the backend and request.
4. `SyncBackend::execute` performs the actual I/O and returns the result.
5. The scheduler resumes the fiber with the result.

Async path:
```
Stream primitive → (SIG_IO, IoRequest) → Scheduler → io/submit → AsyncBackend → OS (async)
                                                    ← io/wait  ← completions ← OS
```

## Key Types

### IoOp

Enum of I/O operations:
- `ReadLine` — read one line (up to `\n`), returns string or nil (EOF)
- `Read { count }` — read up to `count` bytes, returns bytes/string or nil (EOF)
- `ReadAll` — read everything remaining, returns string or bytes
- `Write { data }` — write data to port, returns bytes written (int)
- `Flush` — flush port's write buffer, returns nil

### IoRequest

Struct with `op: IoOp` and `port: Value`. Wrapped as `ExternalObject`
with type_name `"io-request"`. The port is stored as `Value` (not `&Port`)
because the `Value` holds the `Rc` to the `ExternalObject` containing the `Port`.

### SyncBackend

Synchronous backend with per-fd buffering. Wrapped as `ExternalObject`
with type_name `"io-backend"`. Uses `RefCell<SyncBackendInner>` for
interior mutability (ExternalObject wraps in Rc, so `&mut self` is unavailable).

### AsyncBackend

Asynchronous backend with io_uring (Linux, feature-gated) or thread-pool fallback.
Wrapped as `ExternalObject` with type_name `"io-backend"` (same as `SyncBackend`).
Uses `RefCell<AsyncBackendInner>` for interior mutability.

Methods:
- `submit(request) → Result<u64, String>` — submit async I/O, return submission ID
- `poll() → Vec<Completion>` — non-blocking completion poll
- `wait(timeout_ms) → Result<Vec<Completion>, String>` — blocking wait

### BufferPool

Owns `Vec<u8>` allocations indexed by `BufferHandle`. Buffers are allocated on
submit, returned on completion. Lives on `AsyncBackendInner`, not on `FiberHeap`.

### Completion

Returned to Elle as struct: `{:id n :value v :error nil}` (success) or
`{:id n :value nil :error e}` (failure).

## Buffer Drain Invariant

Data already received is never lost when a fd dies (EOF or error).
The backend drains buffered data before surfacing EOF or error status.

State machine:
- **State 1**: Buffer has data, fd alive → read more if needed
- **State 2**: Buffer has data, fd dead → drain buffer first
- **State 3**: Buffer empty, fd dead → return nil (EOF) or error

## Primitives

| Primitive | Effect | Purpose |
|-----------|--------|---------|
| `io-request?` | pure | Check if value is an I/O request |
| `io-backend?` | pure | Check if value is an I/O backend |
| `io/backend` | errors | Create an I/O backend (`:sync` for synchronous, `:async` for asynchronous) |
| `io/execute` | errors | Execute an I/O request on a backend (blocking) |
| `io/submit` | errors | Submit async I/O request, return submission ID |
| `io/reap` | errors | Non-blocking poll for completions (returns tuple) |
| `io/wait` | errors | Blocking wait for completions with timeout (returns tuple) |

## Invariants

1. `IoRequest` values are only created by stream primitives.
2. Backends are only created by `io/backend`.
3. The backend validates port direction and open status before I/O.
4. Stdio ports use `std::io::stdin()/stdout()/stderr()` handles directly
   (Port has no OwnedFd for stdio).
5. Per-fd state is keyed by `PortKey` (Stdin/Stdout/Stderr/Fd(raw_fd)).
6. Buffer drain invariant: buffered data is never lost on EOF or error.
7. Buffers passed to io_uring must not move while the kernel holds them.
   The `BufferPool` on `AsyncBackendInner` owns all async I/O buffers.
8. stdin reads in async mode go through a dedicated OS thread, not io_uring.
9. `io/submit`, `io/reap`, `io/wait` only work with async backends (created via `:async`).
   Passing a sync backend signals a type error.
