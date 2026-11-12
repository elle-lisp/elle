# Writes

<!-- audited: 2026-11-12 -->

How the backend keeps the promise that `port/write` writes every byte of its payload before it returns.

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

## Pinned by

- [port-shortwrite.lisp](../../../tests/lang/port-shortwrite.lisp), on each
  platform, with payloads one syscall provably cannot finish.
