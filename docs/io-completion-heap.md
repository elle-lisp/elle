# I/O Completion Heap Routing

## Problem

When a fiber submits an I/O read, the completion value (string, bytes) is
constructed in `process_raw_completion()` (`src/io/completion.rs`, which
delegates port I/O to `complete_port_op` in `src/io/completion/port.rs`) on
the **scheduler's heap** — not the requesting fiber's heap. The fiber receives a cross-heap pointer via
`fiber/resume`. This has two consequences:

1. **Leak.** Every byte of I/O performed by every fiber accumulates on the
   scheduler's heap until thread death. A server fiber processing 10,000
   HTTP requests holds ~40 MB of dead strings on the scheduler's heap. The
   fiber's scope machinery cannot reclaim them because they're not on its
   heap.

2. **Coupling.** The I/O subsystem allocates Values. When
   `process_raw_completion` runs, the only heap it has in hand is the
   scheduler's — so a completion value is built on the scheduler's heap, not
   the requesting fiber's.

## Solution: Pre-allocated buffer Values

The fiber allocates a buffer Value (`LBytes`) on its own heap before
yielding. The buffer travels with the IoRequest. The kernel fills it. The
completion returns it. No Value construction in the I/O subsystem.

```
Before:
  fiber → yield IoOp::Read{count} → scheduler → backend → kernel
       ← Value::bytes(data) on scheduler heap ← process_raw_completion
       ← fiber/resume(cross-heap pointer)

After:
  fiber → allocate LBytes(N) on own heap
       → yield IoOp::Read{count, buffer} → scheduler → backend → kernel
       ← kernel writes into fiber's bump arena
       ← Completion{buffer} (same Value, already filled)
       ← fiber/resume(own Value)
```

## Detailed flow

### 1. Primitive (`src/primitives/stream.rs`)

```rust
fn prim_stream_read(ctx: &mut NativeCtx, args: &[Value]) -> (SignalBits, Value) {
    let port = extract_port_value(&args[0], "port/read", ctx)?;
    let count = /* parse args[1], must be > 0 */;
    let timeout = extract_keyword_timeout(args, 2, "port/read", ctx)?;
    let buffer = ctx.bytes(vec![0u8; count]);  // ← on fiber's heap, via NativeCtx
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(IoOp::Read { count, buffer }, port, timeout),
    )
}
```

The buffer is allocated via `ctx.bytes(vec![0u8; N])` — a `NativeCtx` method
that places `HeapObject::LBytes` on the fiber's heap, with inline data in the
fiber's bump arena (the fiber's own region). The allocation must go through the
region-aware `NativeCtx`, not a context-free `Value::bytes`, so it lands on the
fiber's own heap.

### 2. Buffer pointer extraction (`src/io/request.rs`)

A single unsafe function encapsulates the writeable-buffer contract:

```rust
/// Extract a writeable pointer and length from a pre-allocated LBytes buffer.
///
/// # Safety
///
/// The caller must ensure that:
/// - The fiber that owns this buffer is parked (no mutator can read the data).
/// - The pointer is used only for a single write (kernel SQE or thread pool copy).
/// - The pointer is not used after the fiber is un-parked or torn down.
pub(crate) unsafe fn writeable_buffer_ptr(buffer: &Value) -> (*mut u8, usize) {
    let ext = buffer.as_heap_ptr().expect("buffer must be heap value");
    let obj = ext as *const HeapObject;
    match &*obj {
        HeapObject::LBytes { data, .. } => {
            (data.as_ptr() as *mut u8, data.len())
        }
        _ => panic!("IoOp buffer must be LBytes, got {}", buffer.type_name()),
    }
}
```

`src/io/request.rs` also defines the companion in-place helpers
`truncate_buffer` (shrink the `RegionSlice` len to the bytes actually
written), `bytes_to_string_in_place` (re-tag `LBytes` → `LString` after
UTF-8 validation, zero-copy), and `set_struct_field_in_place` (used by the
`RecvFrom` completion to stamp `:addr`/`:port` into the pre-allocated result
struct).

The `LBytes` variant stores an `RegionSlice<u8>` — a `(ptr: *const u8,
len: u32)` pair pointing into the fiber's bump arena. We cast away `const`
to write through it. This is safe because:

- Bump arena pages are mmap'd with `PROT_READ | PROT_WRITE`.
- The fiber is parked — no mutator can observe the write.
- The pointer escapes to C (io_uring SQE) — the optimizer cannot assume
  the pointee is unchanged.

All pointer casts are confined to this function. No call site performs
`as_ptr() as *mut u8`.

### 3. Backend submission (`src/io/aio.rs`, `src/io/uring.rs`)

For io_uring reads, extract the writeable pointer and submit:

```rust
IoOp::Read { count, ref buffer } => {
    let (buf_ptr, buf_len) = unsafe { writeable_buffer_ptr(buffer) };
    let submit_len = (count - filled).min(buf_len);
    opcode::Read::new(Fd(fd), buf_ptr, submit_len as u32)
        .offset(u64::MAX)
        .build()
        .user_data(id)
}
```

The kernel writes directly into the fiber's bump arena page. The pointer is
stable — bump arena pages are mmap'd at fixed virtual addresses and never
relocated.

### 4. Short-read re-submission (no copies, no intermediate buffer)

When a non-stream read returns fewer bytes than requested, the re-submission
targets the remaining portion of the same buffer. The `PendingOp` tracks
how many bytes have been filled so far:

```rust
PendingOp::Port {
    op: IoOp::Read { count, ref buffer },
    filled: usize,   // ← bytes written so far
    ..
}
```

On short read (`result_code < count - filled`):

```
filled += result_code;
re-submit with (writeable_ptr + filled, count - filled);
```

Pointer arithmetic into the same RegionSlice. No `BufferPool`, no
`fd_states.buffer` intermediate, no copy. The fiber's buffer is the only
buffer. The final completion returns the buffer Value; the fiber reads the
first `filled` bytes.

### 5. Thread pool fallback (`src/io/threadpool.rs`)

`Value` cannot cross thread boundaries (contains `Rc`), so the fiber's buffer
Value never reaches the worker thread. The `PoolOp::Read`/`ReadLine` variants
carry only the `fd` (and `size` for `Read`):

```rust
pub(super) enum PoolOp {
    Read { fd: RawFd, size: usize },
    ReadLine { fd: RawFd },
    // ...
}
```

The worker thread reads into a local `Vec<u8>` and returns it in
`PoolCompletion { id, result_code, data }`. The bytes are copied into the
fiber's pre-allocated buffer on the **scheduler thread** by the completion
path — `pool_to_completion` (`src/io/aio.rs`) feeds the `PoolCompletion` into
`process_raw_completion` → `complete_port_op` (`src/io/completion/port.rs`),
which obtains the destination pointer via `writeable_buffer_ptr(buffer)` and
`std::ptr::copy_nonoverlapping`s into it, then `truncate_buffer`s to the bytes
written. One extra memcpy on the non-Linux path. There is no separate
`FiberBuffer` type or `copy_to_fiber_buffer` function — the raw pointer is
derived from the buffer Value inside the completion handler, where the fiber
is parked and the Value is on the (single-threaded) scheduler side.

### 6. Completion processing (`src/io/completion/port.rs`)

For read operations, the port completion path returns the pre-allocated
buffer instead of constructing a fresh data Value. `Completion` is built via
the `Completion::ok` / `Completion::new` constructors (not a struct literal),
and the buffer is truncated to the bytes written before being returned. A
binary port returns the `LBytes` buffer as-is; a text port re-tags it to an
`LString` in place via `bytes_to_string_in_place`. (`process_raw_completion`
in `src/io/completion.rs` delegates every `PendingOp::Port` op to
`complete_port_op`.)

```rust
IoOp::Read { ref buffer, .. } | IoOp::ReadExact { ref buffer, .. } => {
    // Data already in buffer's inline slice (io_uring: kernel wrote it;
    // thread pool: the completion handler copied it). Truncate to the
    // bytes written; for a text port re-tag in place, else return as-is.
    unsafe { truncate_buffer(buffer, total); }
    if encoding == Encoding::Text {
        return Completion::new(id, unsafe { bytes_to_string_in_place(*buffer) });
    }
    *buffer  // wrapped in Completion::ok(id, value) at the end of the match
}
```

### 7. Fiber resume

The scheduler calls `(fiber/resume fiber (get c :value))` with the buffer
Value. Since the buffer is on the fiber's own heap, this is an identity
operation — the fiber receives back a Value it created.

### 8. UTF-8 interpretation

Encoding is decided in the completion path from the port's `Encoding`
(`Text` vs `Binary`), via the in-place `bytes_to_string_in_place` re-tag
(no copy — it validates UTF-8 and flips the `LBytes` HeapObject to
`LString`):

- `port/read` / `port/read-exact` on a **binary** port return the `LBytes`
  buffer; on a **text** port they return a string (re-tagged in place).
- `port/read-line` always returns a string (line-boundary detected by the
  backend, then `bytes_to_string_in_place`), or `nil` at EOF.
- A UTF-8 error surfaces as an `encoding-error` from `bytes_to_string_in_place`.

The buffer still lives on the fiber's heap throughout, so the re-tagged
string is in the fiber's own region — no cross-heap pointer.

## Changes by file

### `src/io/request.rs`

- `IoOp::Read { count }` → `IoOp::Read { count: usize, buffer: Value }`
- `IoOp::ReadLine` → `IoOp::ReadLine { buffer: Value }`
- `IoOp::ReadExact { count: usize, buffer: Value }` — the "exactly N units"
  variant; also pre-allocated (a text port reserves 4 bytes per grapheme).
- `IoOp::ReadAll` — stays a unit variant (no pre-allocated buffer); ReadAll
  is unbounded and accumulates in `fd_states.buffer` (see below).
- Add `unsafe fn writeable_buffer_ptr(buffer: &Value) -> (*mut u8, usize)`
  plus the in-place helpers `truncate_buffer`, `bytes_to_string_in_place`,
  and `set_struct_field_in_place`, all with full safety documentation.

### `src/primitives/stream.rs`

- `prim_stream_read`: allocate `ctx.bytes(vec![0u8; count])`, include in
  IoOp.
- `prim_stream_read_line`: allocate `ctx.bytes(vec![0u8; 65536])`,
  include in IoOp. 64KB covers every real protocol line. If a line exceeds
  this, the fiber receives a partial result and re-issues the read.
- `prim_stream_read_all`: no pre-allocated buffer. ReadAll is unbounded and
  defers to the existing `fd_states.buffer` accumulation path (scheduler
  heap). ReadAll is called once per file, so the leak is bounded by file
  count.

### `src/io/uring.rs`

- `submit_uring_stream`: for Read/ReadLine, extract writeable pointer from
  IoOp's buffer field via `writeable_buffer_ptr`. No `BufferPool::alloc()`.
- `drain_cqes`: for read completions, return the pre-allocated buffer.
  Short-read re-submission targets `writeable_ptr + filled` with
  `count - filled` — pointer arithmetic into the same buffer, no copy.

### `src/io/completion.rs` / `src/io/completion/port.rs`

- `process_raw_completion` delegates every `PendingOp::Port` op to
  `complete_port_op` (`src/io/completion/port.rs`).
- `complete_port_op`: for Read/ReadLine/ReadExact, truncate and return the
  pre-allocated buffer from the PendingOp's IoOp rather than constructing a
  fresh data Value. Encoding branching is *retained* (not removed): a text
  port re-tags the buffer to `LString` in place via
  `bytes_to_string_in_place`; a binary port returns the `LBytes` buffer.
- ReadAll: accumulates in `fd_states.buffer` and constructs the result via
  `ctx.bytes(data)` (region-aware) — still on the scheduler heap.

### `src/io/aio.rs`

- `AsyncBackend::submit`: for Read/ReadLine, skip `buffer_pool.alloc()`.
  Extract buffer pointer via `writeable_buffer_ptr`.
- ReadLine/Read buffered-fast-path: copy buffered data into the
  pre-allocated buffer Value.
- `stdin_to_completion`: copy stdin thread data into the pre-allocated
  buffer.
- ReadAll: unchanged from current path.

### `src/io/threadpool.rs`

- `PoolOp::Read { fd, size }`, `PoolOp::ReadLine { fd }`: carry no buffer —
  the worker reads into a local `Vec<u8>` returned via `PoolCompletion.data`.
  The completion handler (on the scheduler thread) copies that data into the
  fiber's pre-allocated buffer via `writeable_buffer_ptr`. No `FiberBuffer`
  type and no BufferPool slot for these ops.
- `PoolOp::ReadAll`: unchanged from current path.

### `src/io/pending.rs`

- `PendingOp::Port.buffer_handle` → `Option<BufferHandle>`. Read/ReadLine
  set `None` (no BufferPool slot). Non-read operations set `Some(...)`.
- Add `filled: usize` field for short-read cursor tracking.
- `buffer_handle()` returns `Option<BufferHandle>`; callers match on it.

## Interaction with existing mechanisms

### Per-fd ReadLine buffering

The backend's `fd_states[port_key].buffer` stores over-read bytes from
previous reads. When serving a ReadLine from the buffer (no kernel read
needed), copy the line bytes into the pre-allocated buffer Value. Same
abstraction as the kernel path — the fiber gets its buffer back, filled.

### ReadLine re-submission

When the kernel returns data without a newline, `drain_cqes` re-submits
another read into the remaining portion of the same fiber buffer
(`writeable_ptr + filled`, remaining capacity). No intermediate buffer, no
copy. If the buffer fills (64KB) without a newline, the completion returns
the partial buffer to the fiber. The fiber can re-issue `port/read-line` to
continue reading.

### Short-read re-submission

For non-stream file reads that return short, the re-submission advances the
`filled` cursor and targets the remaining buffer space. Pointer arithmetic
into the same RegionSlice — no copy, no `fd_states.buffer` intermediate.

### ReadAll (deferred)

ReadAll loops until EOF, accumulating in `fd_states.buffer`. The final Value
is constructed via `ctx.bytes(data)` on the scheduler heap. This is the
one read path that still allocates on the scheduler's heap. ReadAll is
typically called once per file (not in a loop), so the leak is bounded by
file count, not iteration count. The optimization can be deferred.

### SharedAllocator / Outbox

Pre-allocated buffers live on the fiber's heap. If the fiber has a
SharedAllocator (yielding child), the buffer is on the SharedAllocator's
pool — which is the parent's pool. The parent can read it directly.
No conflict with existing inter-fiber routing.

## Buffer lifetime safety

The fiber allocates the buffer before yielding. The fiber is parked while
the I/O is in flight. The fiber's heap is not torn down (parked, not dead).
The bump arena page containing the buffer's inline data remains valid.
The kernel writes into a stable mmap'd page.

If the fiber is cancelled (killed) while I/O is pending:
- The scheduler cancels the pending I/O operation (`io/cancel`).
- The fiber's heap is torn down (`FiberHeap::clear()`).
- The kernel's SQE is cancelled before the fd is closed.
- The buffer's inline data is freed with the rest of the fiber's heap.
- No dangling kernel write — the cancellation ensures the kernel won't
  touch the buffer after cancellation.

**Requirement**: the scheduler must cancel pending I/O before tearing down
a parked fiber. This is already the behavior in `do-shutdown`.

## What this removes

1. Fresh data-Value construction (`ctx.bytes(...)` / string allocation) in the
   completion path for Read/ReadLine/ReadExact operations — the pre-allocated
   buffer is returned (re-tagged in place for text ports) instead.
2. Cross-heap pointers from scheduler to fiber for read completions.
3. Unbounded scheduler-heap accumulation from read-heavy fibers (Read/ReadLine).
4. `BufferPool` allocation for Read/ReadLine operations.
5. Intermediate buffers (`fd_states.buffer` as accumulator) for bounded reads.

## What this does not change

- Write operations — no buffer pre-allocation needed; the fiber provides
  data Values as today.
- Accept, Connect, Open — these create new ports, not read buffers.
- Flush, Shutdown, Sleep, PollFd, Resolve, WatchNext — no read data.
- Spawn, ProcessWait — already use `origin_heap` for heap routing.
- ReadAll — still accumulates in `fd_states.buffer`, constructs Value on
  scheduler heap. Deferred optimization.
- The `BufferPool` — still used for non-read operations (write data,
  RecvFrom sockaddr, etc.) and for ReadAll accumulation.

## Test strategy

1. **Unit test:** Create `Value::bytes(vec![0u8; 16])`, call
   `writeable_buffer_ptr`, write through the pointer, read back via
   `as_bytes()`. Verify inline-slice write-through.
2. **Integration test:** Read from a known file via both uring and thread
   pool paths. Verify returned bytes match expected content.
3. **Leak test:** Read 10,000 times from a port in a tight loop. Verify the
   fiber's heap does not grow beyond one buffer's worth of live data.
4. **Short-read test:** Read from a source that returns short reads. Verify
   the cursor advances correctly and the final buffer contains all data.
5. **ReadLine test:** Read lines from a source with and without newlines.
   Verify line boundary detection with pre-allocated buffers.
6. **Cancel test:** Cancel a fiber while a read is pending. Verify no
   use-after-free, no panic.
7. **Encoding test:** Verify `port/read` returns `bytes` on a binary port and
   a string on a text port, and that the text path's in-place
   `bytes_to_string_in_place` re-tag produces correct strings (and an
   `encoding-error` on invalid UTF-8).
