# I/O Module

## Purpose

The `io` module contains I/O request types and backends for Elle's
scheduler-based I/O system. Stream primitives build `IoRequest` values
and yield them via `SIG_IO`. The scheduler catches `SIG_IO` and dispatches
to a backend for execution.

## Modules

| Module | Responsibility |
|--------|----------------|
| `request.rs` | `IoRequest` and `IoOp` types — typed I/O request descriptors |
| `backend.rs` | `SyncBackend` — synchronous I/O execution with per-fd buffering |

## Data Flow

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
| `io/backend` | errors | Create an I/O backend (`:sync` for synchronous) |
| `io/execute` | errors | Execute an I/O request on a backend (blocking) |

## Invariants

1. `IoRequest` values are only created by stream primitives.
2. `SyncBackend` is only created by `io/backend`.
3. The backend validates port direction and open status before I/O.
4. Stdio ports use `std::io::stdin()/stdout()/stderr()` handles directly
   (Port has no OwnedFd for stdio).
5. Per-fd state is keyed by `PortKey` (Stdin/Stdout/Stderr/Fd(raw_fd)).
6. Buffer drain invariant: buffered data is never lost on EOF or error.
