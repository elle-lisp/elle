# The two platforms

<!-- audited: 2026-11-11 -->

io_uring or a thread pool runs each operation, and each platform has one blocking wait that hears every completion.

## Which platform runs

A Linux build with the `uring` feature opens a 256-entry ring, and takes the
thread pool when the ring will not open. Every other build takes the pool.
`AsyncBackend::new_with_unicode` makes the choice
([aio.rs](../../../src/io/aio.rs)), and `PlatformBackend` records it.

Some work has no form on the ring, so it runs on the pool on both platforms:

| Operation | Why it runs on the pool |
|-----------|------------------------|
| `Resolve` | `getaddrinfo(3)` has no ring form |
| `Task` | an arbitrary closure has no ring form |
| `Open`, `File` | see below |
| a read of stdin | the stdin worker, a thread of its own, reads descriptor 0 |

An `Open` and every `File` operation run on the pool because an `open(2)` of a
fifo waits for the other end. A wait is answerable only where the worker holds
it, and `IORING_OP_OPENAT` blocks an io-wq thread that a linked timeout marks
cancelled but cannot take back. One implementation also gives one answer: a
fifo behaves as [files](../../io/files.md) says whichever platform is
underneath.

A sync opens its path with `O_NONBLOCK`, so the open of a fifo returns at once.
The sync then refuses with `EINVAL` anything but a file or a directory before
it calls `fsync(2)`, which refuses a fifo on Linux and syncs one on macOS. The
other file operations run through blocking `std::fs` calls that nothing
interrupts, so on a fifo they hold their worker until the other end opens. They
hold no scheduler, and other fibers run meanwhile. Pinned by
[openfile.rs](../../../src/io/threadpool/tests/openfile.rs),
[file.rs](../../../src/io/threadpool/tests/file.rs),
[file-sync.lisp](../../../tests/lang/file-sync.lisp) and
[file-content.lisp](../../../tests/lang/file-content.lisp).

## Operations that finish inside `io/submit`

These run no platform at all. The submit call pushes their completion onto the
queue and files no pending entry:

- `Spawn`, because the child starts in the call
  ([waiting on a child](process.md)).
- `ProcessWait` on a child whose exit status the handle already holds.
- `Close`, `Seek` and `Tell`. A seek and a tell call `lseek(2)` in the submit
  path, and `PoolOp` has no variant for either.
- `Flush` on a socket, a pipe, stdout or stderr, where `fsync(2)` answers
  `EINVAL`.
- A read the port's remainder answers in full
  ([where a stream operation's bytes live](bytes.md)).

## One blocking wait per platform

Every background worker reports through one channel, the `CompletionHub`: the
pool's workers and the stdin worker alike. A worker cannot build a
`Completion`, because building one needs the backend's own tables and heap, so
it sends its raw result, and the scheduler thread cooks it.

On the pool platform the scheduler's blocking wait is a `recv()` on that
channel. A crossbeam receiver registers before it sleeps, so a worker's publish
cannot be missed.

On the ring the scheduler blocks in one `io_uring_enter`. Hub work posts no CQE
of its own, so a standing `POLL_ADD` on an eventfd bridges the two. A worker
raises the eventfd after it publishes, so the wake never comes before the item.
The wait clears the eventfd and arms the one-shot poll again. The poll's
CQE carries the sentinel `EVENTFD_USER_DATA`, which no minted id can reach.

Linux 6.15 and later take `IORING_ENTER_NO_IOWAIT` on that wait, and
`Ring::open` asks the kernel once whether it does. Without the flag the kernel
counts a wait on a ring with a request in flight as iowait. The bridge poll is
always in flight, so an idle program parked on a socket or a timer would read
as waiting for disk in `top` and `vmstat`. Pinned by
[tests.rs](../../../src/io/uring/ring/tests.rs).

Either way there is one blocking primitive, and no wait is capped to rescue a
lost wakeup. A wakeup that is truly lost hangs the loop rather than turning
into a stall nobody notices. The timeout a wait passes to the platform is
always the caller's.

## The submission frame

Every operation the backend issues goes through `AsyncBackend::submit_op`
([requests.rs](../../../src/io/aio/requests.rs)), in four steps:

| Step | What it decides |
|------|-----------------|
| `mint_id` | the id the operation carries to the kernel or to a worker |
| `buffer_pool.alloc(buf_bytes)` | the pinned bytes the kernel may write into, held until the completion arrives |
| `dispatch(&mut Dispatch)` | which platform runs the operation, and what its pending entry must remember |
| `pending.insert(id, ..)` | the entry the arriving completion resolves through |

The `match platform` stays at each call site. Every operation calls its own
`submit_uring_*` and builds its own `PoolOp`, and the ring type does not exist
off Linux, so a shared helper would need a `#[cfg]` on its signature.
`Dispatch::poll_fd` is the exception: `ev/poll-fd` and the `chan/wait-ready`
park wait on a bare descriptor the same way. Both watch a descriptor somebody
else owns without changing it, so the pool worker takes `OpBound::watching`.

A dispatch failure returns before any pending entry exists, and leaves the
buffer reserved. `submit_linked` can fail with the operation's SQE already on
the submission queue, so the kernel may still read that buffer on the next
`ring.submit()`.

The byte-stream operations go through `submit_stream` rather than
`submit_op`, because a read lends the port's remainder and a write hands over
its payload by address ([where a stream operation's bytes live](bytes.md)).
Each still files one entry under the id it dispatched with.

## One submission, one entry, one id

An entry is filed under the id its operation was dispatched with, so an
arriving completion always finds its entry. A completion that finds no entry is
discarded, and the fiber waiting on it never wakes: a hang, not an error.

The arm an entry selects decides who owns the completion's payload.
`ProcessWait` reclaims a `Box<siginfo_t>`, `Connect` and `Open` take ownership
of a descriptor, and the port arms write through a fiber's buffer. So a
completion that resolves to the wrong entry applies one operation's ownership
rules to another operation's payload.

A pool worker therefore reports the `OpKind` it ran beside the id, and
`pool_to_completion` asks `PendingOp::accepts` whether an operation of that
kind could have filed the entry. A "no" is the table and the worker
contradicting each other about one id. The completion is then withheld: the
entry is let go **unread** rather than retired, because retiring reclaims the
very payload in question, and the fiber is told what happened. The kinds are
coarser than `PendingOp` because they name what a worker can report having
done: `ev/poll-fd` and the `chan/wait-ready` park run the same operation, so
both answer to `OpKind::Poll`.

The stdin worker runs reads on a port and nothing else, so its completions
answer to `OpKind::Port`. io_uring has no such tag: a CQE's `user_data` is the
`u64` the SQE carried, returned by the kernel untouched.

Pinned by `a_completion_for_another_operation_is_withheld_from_the_entry_it_found`
and `a_pending_entry_accepts_only_the_kind_of_operation_that_filed_it`, with the
rest of the frame, in [submit.rs](../../../src/io/aio/tests/submit.rs).
