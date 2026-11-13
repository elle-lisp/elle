# Writes

<!-- audited: 2026-11-12 -->

How the backend sends every byte of a write before it returns, and runs one write per port at a time.

[Writing to a port](../../io/write.md) states the promise a program sees.

## A write runs to the end of its payload

`PortOp::Write` completes only when the whole payload has left for the
descriptor. One `write(2)` transfers at most what fits in the send buffer, so
both platforms loop:

| Platform | How it loops |
|----------|--------------|
| io_uring | `after_write` ([resubmit.rs](../../../src/io/uring/resubmit.rs)) resubmits the unwritten tail from the same payload address, and `PendingOp::Port.filled` counts the bytes already accepted |
| thread pool | the worker's `write` ([stream.rs](../../../src/io/threadpool/stream.rs)) loops `writev(2)` inside the one operation |

The completion reports `filled + result_code`, which equals the payload's
length. A `port/write-many` payload is its parts, and its tail starts inside
whichever part `filled` reaches. A write that moves nothing of a non-empty tail
fails with `EIO`, because resubmitting it would spin.

A failure part of the way through surfaces as an error, not as a short count.
A count smaller than the payload would read as success to a caller that trusts
the promise.

Each pass of the loop waits under the caller's `:timeout`, armed again after
every transfer. So a peer that stops reading trips the timeout, while a slow one
lets the write finish ([how an operation is bounded](bounds.md)). Where the
payload's bytes live while the kernel reads them is
[where a stream operation's bytes live](bytes.md).

## One write at a time per port

Nothing below the backend orders two writes on one descriptor. On the pool two
workers can each be inside `writev(2)` on it at once. On the ring a short write
resubmits its tail, and a second write's SQE can reach the kernel between the
two. Either way the peer receives runs of each payload in turn. So the backend
lets one write per port run, and the rest wait in line. A `port/write` and a
`port/write-many` are both a `PortOp::Write`, and they share the one line.

`PendingTable` keeps a line per `PortKey`: the id of the running write, and a
queue of the writes behind it. The three stdio ports have keys, so `*stdout*`
and `*stderr*` are lined up too. A write starts at once when its port has no
line. Otherwise it waits at the back.

A waiting write has its id, and the scheduler pairs the id with its fiber as
for any submission. It is not in flight: its entry is not among the operations
`take`, `ids` and `orphaned_to_stop` read. It holds its operands in an
`OperandHold`, as a filed entry does. It takes no share of the descriptor and
no pooled buffer, because it reaches neither until it starts.

A write whose bytes must be copied takes its copy when it joins the line,
because the copy is the bytes the call was given
([where a stream operation's bytes live](bytes.md)). A part that another fiber
changes while the write waits in line does not change what is sent. The write
hands that copy to its submission when it starts, and the submission copies
nothing more.

`drain_ready` ends by starting the next write of every line whose running id
has left the table. A short write on the ring keeps its entry across the
resubmission, so its tail goes out before the next write starts. So does a
gathered write of more than `IOV_MAX` parts, which goes out as several
syscalls inside the one operation. A cancelled
write keeps its entry until its completion is reaped, so the next write never
races the cancelled one's last syscall. A start finds the port as the write
left it. A closed port answers the write with an error completion, and the
next write in line starts after it.

| Event | A waiting write |
|-------|-----------------|
| `io/cancel` | leaves its line, sends nothing, and delivers no completion |
| its fiber ends by a route the scheduler did not take | leaves its line, and answers with `PendingTable::orphaned_asker_error` |
| its `:deadline` passes | leaves its line, sends nothing, and answers with a `:timeout` error |
| backend teardown | leaves its line, and its hold is let go |

A waiting write's `:timeout` starts when the write starts
([I/O deadlines](../../io/timeout.md)). Its `:deadline` needs a wake of its own,
because no completion may come before it passes. `wait` therefore blocks no
longer than the earliest deadline among the waiting writes.

Reads are not lined up: two fibers that read one port divide its bytes between
them however the reads land. Neither are two backends that write one
descriptor, such as a nested `ev/run` that makes a backend of its own.

## Pinned by

- [port-shortwrite.lisp](../../../tests/lang/port-shortwrite.lisp), on each
  platform, with payloads one syscall provably cannot finish.
