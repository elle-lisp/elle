# An operation in flight

<!-- audited: 2026-09-16 -->

What a submitted I/O operation holds while the kernel works, how it ends when
the fiber that asked is gone, and how its answer is assembled.

Up: [io/](../../src/io/AGENTS.md)

## A submitted operation holds the values its completion reads

`PendingOp` holds `Value`s the completion dereferences when it assembles a
result: the port an operation names, the buffer or result struct the caller
reserved, the payload a write hands over, the process handle, the watcher, the
receiver. None of them are the operation's own allocations. Each was born in a
region of the fiber that asked, and the pending table is runtime-side state that
no free-time cascade reaches — the same position a message in a channel buffer
is in (the `Sends` variant in [region effects](region/effects.md)).

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

## A hold retains what reclamation listens to

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

## A hold is let go while its store is still there

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

## An operation whose fiber is gone has no reader

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

## Ending an operation whose fiber is gone

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
and be resumed again (see [fiber primitives](../signals/primitives.md)),
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

## The stop pipe

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

`read-exact` on the thread pool is the one exception, and it is one because
that op will not answer short. A worker asked for the whole count would wait
for the remainder a second time, from a peer that has already sent it once. So
the submission hands the remainder to the worker (`PoolOp::ReadExact`'s `held`)
and the worker reads only the shortfall. The ring reaches the same count
through its resubmit test instead (`src/io/uring/drain.rs`), and either way
`assemble_read` still joins what comes back.

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

