# Where a stream operation's bytes live

<!-- audited: 2026-09-23 -->

A read lands in the caller's region and a write leaves from the payload's, and the operation holds both while the kernel works.

Up: [io/](../../src/io/AGENTS.md)

## A read lands in the caller's buffer

`port/read`, `port/read-line` and `port/read-exact` reserve their answer before
they yield: an `LBytes` in the region the call minted for its result. Both
backends hand the kernel that buffer's address. The ring puts it in the SQE, and
a pool worker passes it to `read(2)`. So the kernel moves each byte once, into
the region, and nothing in user space moves it again.

The answer is that same buffer, truncated to the bytes that answer the request.
On a text port it is transmuted to a string in place. A byte past the answer —
beyond the newline, or past the Nth cluster — goes to the port's remainder
(`FdState`). The remainder is a Rust `Vec`, because it outlives this call's
region and belongs to the next read on the port.

The reservation itself is zeroed in the region. Building it from a `Vec` of
zeroes would stage the whole reservation on the Rust heap first.

## The port hands its remainder to the read

A read that the port's remainder cannot answer on its own still owns those
bytes: they come first in stream order. The submission moves the remainder out
of the port and into the operation, and copies it into the front of the caller's
buffer. The kernel then reads into the buffer behind it, so the bytes the answer
is cut from lie together in the region. A cluster that straddles the remainder
and the new bytes is counted where it lies.

The operation owns the remainder until it answers. A read that ends without
answering gives the remainder back to the front of the port's remainder:

- A cancel gives it back at once, not when the completion arrives. The next
  read on the port is often submitted before the cancelled one is reaped — a
  lost `ev/timeout` followed by another `port/read` — and it must see those
  bytes first.
- A read whose fiber is gone gives it back when its entry is retired. The port
  can still have another reader.
- A read that fails gives it back before it reports the error. A read that
  timed out has taken nothing from the stream.

The remainder moves rather than being copied, so each byte has one owner. A
second read submitted while the first is in flight finds no remainder to answer
from. It cannot hand out bytes the first read will also answer with.

A remainder as large as the buffer stays with the port. The completion joins it
to the new bytes outside the buffer, as the next section describes.

## When the answer outgrows the buffer

What a read reserves is not a bound on what it answers with. A text
`read-exact` counts grapheme clusters, and a cluster is any number of joined
codepoints. So a read can need more bytes than any reservation holds.

When the buffer fills before a text `read-exact` has its count, the bytes move
to a `Vec`. On the ring that `Vec` is the port's remainder. On the pool it is
the worker's own. The completion then joins the parts and builds the answer at
its `Birthplace`, exactly as `read-all` does. Clamping to the buffer would
drop the bytes past it. The port has already taken those bytes from the kernel,
so nothing would be left to read them again.

A `read-line` whose buffer fills with no newline answers with the full buffer.
Reading on gives the next piece. Both backends answer this way, so a line longer
than 64 KiB arrives in the same pieces on either one.

The submission answers from the remainder alone whenever it can, using
`frame::line_end` and `frame::exact_end`. The completion cuts with the same
two, so the submission and the completion cannot frame one stream two ways.
Answering there is not merely a saved syscall. A read submitted for bytes the
port is already holding would wait for the peer to send more, and a peer that
has said everything never will.

## `read-all` copies once

`read-all` cannot reserve its answer, because its length is unknown until the
stream ends. The bytes accumulate in one Rust buffer the operation owns: a
pooled buffer on the ring, the worker's own on the pool. The kernel reads
straight into that buffer. On a regular file the buffer is sized to the rest of
the file before the first read, so it never grows.

At the end of the stream the completion copies the bytes once into a region at
its `Birthplace`, with the port's remainder in front. A region slice cannot grow
in place, so this one copy is what an unknown length costs.

The accumulation is not the port's remainder. A `port/close` discards the
remainder, and the kernel may be writing into the accumulation at that moment.
A buffer the operation owns goes away only with the operation.

## A write leaves from the payload

An immutable string or bytes payload lives in region pages. Both backends hand
the kernel its address: the ring in the SQE, and in each resubmission of the
unwritten tail; a pool worker in its `write(2)`. No byte of it is copied.

Three payloads are copied once instead. A mutable `@string` or `@bytes` keeps
its bytes in a Rust `Vec` that another fiber can grow, and a reallocation would
move them under the kernel. Any other value is written as its display text,
which exists only once it is formatted. And a pool operation that carries no
stop pipe copies, for the reason in the last section.

## The kernel's operand is held until the completion arrives

The operand hold ([an operation in flight](io-inflight.md)) retains a read's
buffer and a write's payload like every other operand. A cancel lets go of what
only the completion reads: the port and the fiber that asked. It keeps the one
operand the kernel or a worker addresses — the buffer, the payload, or a
`recv-from` result whose `:data` the ring receives into.

A cancel asks the operation to stop. Until the kernel or the worker reports
that it has, a read may still write into the buffer and a write may still read
the payload. A region let go at the cancel can be reclaimed and handed out
again. A late read would then write into another value's memory, and a late
write would send that value to the peer.

The cost that releasing at the cancel avoids is not paid here. A timer has no
addressed operand, so an `ev/timeout` loop holds nothing more. A cancelled read
or write answers at the next drain, and its hold goes with that completion.

## A worker that addresses a region is waited for at teardown

A pool worker that reads into a buffer or writes from a payload is in the
position the ring's kernel is in. Its bytes land in, or leave from, a region
that the heap frees at teardown. So a backend coming to rest stops every pool
operation that carries a stop pipe and waits for its completion before it lets
the holds go. The wait is bounded, as the ring's drain is.

A pool operation addresses the region only when it carries a stop pipe. When
the process is out of descriptors there is no pipe, and nothing could end the
operation at teardown. The worker then reads into a buffer of its own and
writes from a copy, and the completion copies once. So every operation the
teardown waits for is one it can end.

The stdin worker always reads into a buffer of its own. It serves a descriptor
the whole process shares, and its teardown is not a backend's.

## Pinned by

- [io_copies.rs](../../tests/io_copies.rs) and
  [io_copies_pool.rs](../../tests/io_copies_pool.rs) count the Rust heap
  bytes a read, a write and a `read-all` allocate, one binary per backend. The
  count must not grow with the bytes moved, except for the one copy
  `read-all` makes.
- `a_write_hands_the_kernel_the_payload_where_it_lies`
  ([bytes.rs](../../src/io/aio/tests/bytes.rs)) changes a payload after its
  write parks. The peer receives the changed bytes.
- `a_read_that_ends_unanswered_gives_the_port_its_remainder_back`
  ([bytes.rs](../../src/io/aio/tests/bytes.rs)) cancels a read and times one
  out, and reads the remainder afterwards.
- `a_pool_teardown_waits_for_the_workers_that_address_a_region`
  ([bytes.rs](../../src/io/aio/tests/bytes.rs)).
- `a_cancelled_entry_keeps_what_the_kernel_addresses`
  ([hold.rs](../../src/io/pending/tests/hold.rs)).
- [port-longline.lisp](../../tests/elle/port-longline.lisp) and
  [port-text-framing.lisp](../../tests/elle/port-text-framing.lisp), on the
  other backend by `port_longline_threadpool` and
  `port_text_framing_threadpool`
  ([modes.rs](../../tests/integration/elle_scripts/modes.rs)).
