# The I/O backend

<!-- audited: 2026-11-11 -->

How this implementation runs a program's I/O: the request a primitive yields, the backend that runs it, and the answer that comes back.

[I/O](../../io.md) says what a program sees. The documents here say how the
Rust backend in [src/io](../../../src/io/mod.rs) delivers it. Another
implementation of Elle brings a backend of its own and owes the program only
what that document promises.

## A request goes down and a completion comes back

```text
primitive → (SIG_IO, IoRequest) → scheduler → io/submit → backend → OS
                                            ← io/wait   ← completions ← OS
```

A primitive that does I/O runs no syscall. It builds an `IoRequest` and
returns it with `SIG_IO`, which suspends the calling fiber
([stream.rs](../../../src/primitives/stream.rs)). The scheduler in
[the standard library](../../../src/stdlib.lisp) hands the request to
`io/submit`, which answers a submission id. `io/wait` answers the completions
that have arrived, each a struct `{:id n :value v :error e}`, and the scheduler
resumes the fiber the id belongs to.

Only a primitive builds an `IoRequest`. A backend is built by `io/backend`, or
by the WASM host for the I/O a compiled module asks for at its top level, where
no scheduler takes it ([the host's I/O](../../../src/wasm/host/io.rs)). The
`io/` primitives that drive a backend take any `IoBackend`, so they reach
the async backend and the in-memory mock one alike
([io.rs](../../../src/primitives/io.rs)).

## The request

`IoRequest` carries an `IoOp`, the port or external the operation names, the
`Bound` it waits under ([I/O deadlines](../../io/timeout.md)), and the fiber
whose park raised it. The request counts neither the port nor the value its
operation names, because both belong to that park
([what a park retains](../region/park.md)).

`IoOp` has three kinds of variant:

| Kind | Variants | What the backend does |
|------|----------|----------------------|
| An operation on an open port | `Port(PortOp)` | runs it, and files a `PendingOp::Port` entry until it completes |
| A port operation that finishes at once | `Close`, `Seek`, `Tell` | answers inside `io/submit` |
| Portless | `Connect`, `Sleep`, `Spawn`, `ProcessWait`, `Open`, `File`, `Task`, `Resolve`, `WatchNext`, `SigNext`, `PollFd`, `ChanSelectPark` | dispatches before it looks for a port |

The portless operations are dispatched before the port guard because their
port field holds something else, or nothing: a `ProcessWait` carries the
`subprocess` value there, a `WatchNext` its watcher, and a `Spawn` nil. A
`Connect` carries a parsed IP address. Name resolution is the job of the
standard library's `tcp/connect`, which calls `sys/resolve` and then
`tcp/connect-ip` for each address, so the backend never resolves a name to
connect.

## Ports and their state

A port operation is keyed by `PortKey`, which names the port's descriptor
together with the port instance ([types.rs](../../../src/io/types.rs)). The
three stdio ports own no descriptor. The backend writes to descriptors 1 and 2
by number, and a read of descriptor 0 goes to a worker thread of its own
([the platforms](platforms.md)).

The backend refuses an operation on a closed port with `port is closed`. It
leaves the port's direction to the kernel: a write on a port opened for
reading fails with the kernel's `EBADF`.

A read that takes bytes past its answer leaves them with the port, and the next
read on that port answers from them first. Buffered bytes are never lost: the
backend hands them over before it reports the end of the stream or an error
([where a stream operation's bytes live](bytes.md)).

## Network operations

A connect, an accept, a send and a receive yield, as every other I/O call does.
Setting up a socket does not: `tcp/listen`, `udp/bind` and `unix/listen` finish
inside the call.
