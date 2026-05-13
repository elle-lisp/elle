# I/O Completion Heap Routing

## Problem

When a fiber submits an I/O read, the completion value (string, bytes) is
constructed in `process_raw_completion()` on the **scheduler's heap** — not
the requesting fiber's heap. The fiber receives a cross-heap pointer via
`fiber/resume`. This has two consequences:

1. **Leak.** Every byte of I/O performed by every fiber accumulates on the
   scheduler's heap until thread death. A server fiber processing 10,000
   HTTP requests holds ~40 MB of dead strings on the scheduler's heap. The
   fiber's scope machinery cannot reclaim them because they're not on its
   heap.

2. **Coupling.** The I/O subsystem allocates Values. It calls
   `Value::string()`, `Value::bytes()`, `Value::struct_from()`. These
   allocate on whatever `CURRENT_FIBER_HEAP` is installed when
   `process_raw_completion` runs — which is the scheduler's heap.

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
fn prim_stream_read(args: &[Value]) -> (SignalBits, Value) {
    let port = extract_port_value(&args[0])?;
    let count = args[1].as_int().filter(|&n| n > 0)?;
    let timeout = extract_keyword_timeout(args, 2)?;
    let buffer = Value::bytes(vec![0u8; count as usize]);  // ← on fiber's heap
    (
        SIG_YIELD | SIG_IO,
        IoRequest::with_timeout(IoOp::Read { count: count as usize, buffer }, port, timeout),
    )
}
```

The buffer is a normal `Value::bytes(vec![0u8; N])` — `HeapObject::LBytes`
on the fiber's slab, inline data in the fiber's bump arena. The allocation
happens during the fiber's execution, so `CURRENT_FIBER_HEAP` is the fiber's
own heap.

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
    let ext = buffer.as_heap_object().expect("buffer must be heap value");
    match ext {
        HeapObject::LBytes { data, .. } => {
            (data.as_ptr() as *mut u8, data.len())
        }
        _ => panic!("IoOp buffer must be LBytes"),
    }
}
```

The `LBytes` variant stores an `InlineSlice<u8>` — a `(ptr: *const u8,
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

Pointer arithmetic into the same InlineSlice. No `BufferPool`, no
`fd_states.buffer` intermediate, no copy. The fiber's buffer is the only
buffer. The final completion returns the buffer Value; the fiber reads the
first `filled` bytes.

### 5. Thread pool fallback (`src/io/threadpool.rs`)

`Value` cannot cross thread boundaries (contains `Rc`). The `PoolOp`
carries a raw pointer extracted at submit time:

```rust
/// A writeable pointer into a parked fiber's bump arena.
///
/// Safe because the fiber is parked for the duration of the I/O operation.
struct FiberBuffer(*mut u8, usize);
```

The worker thread reads into a local `Vec<u8>` (as today). The completion
handler on the scheduler thread copies the Vec's bytes into the fiber's
buffer via the raw pointer. One extra memcpy on the non-Linux path.

```rust
fn copy_to_fiber_buffer(fb: FiberBuffer, data: &[u8]) {
    unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), fb.0, data.len()) };
}
```

### 6. Completion processing (`src/io/completion.rs`)

For read operations, `process_raw_completion` returns the pre-allocated
buffer instead of constructing a new Value. No `Value::string()` or
`Value::bytes()` call.

```rust
IoOp::Read { ref buffer, .. } => {
    // Data already in buffer's inline slice (io_uring: kernel wrote it;
    // thread pool: copy_to_fiber_buffer wrote it). Return the buffer as-is.
    Completion { id, result: Ok(*buffer) }
}
```

### 7. Fiber resume

The scheduler calls `(fiber/resume fiber (get c :value))` with the buffer
Value. Since the buffer is on the fiber's own heap, this is an identity
operation — the fiber receives back a Value it created.

### 8. UTF-8 interpretation

The I/O subsystem delivers bytes, period. Port encoding is no longer the
I/O subsystem's concern. UTF-8 interpretation happens in fiber-side code
(stdlib wrappers or user code):

- `port/read` returns `bytes` (the LBytes buffer).
- `port/read-line` returns `bytes` (the LBytes buffer, line-boundary
  detected by the backend).
- Fiber-side code converts bytes → string if needed.

This causes UTF-8 errors to surface from the fiber's code — the right stack
frame.

## Changes by file

### `src/io/request.rs`

- `IoOp::Read { count }` → `IoOp::Read { count: usize, buffer: Value }`
- `IoOp::ReadLine` → `IoOp::ReadLine { buffer: Value }`
- `IoOp::ReadAll` → `IoOp::ReadAll { buffer: Value }`
- Add `unsafe fn writeable_buffer_ptr(buffer: &Value) -> (*mut u8, usize)`
  with full safety documentation.

### `src/primitives/stream.rs`

- `prim_stream_read`: allocate `Value::bytes(vec![0u8; count])`, include in
  IoOp.
- `prim_stream_read_line`: allocate `Value::bytes(vec![0u8; 65536])`,
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

### `src/io/completion.rs`

- `process_raw_completion`: for Read/ReadLine, return the pre-allocated
  buffer from the PendingOp's IoOp. Remove Value construction for these
  operations. Remove port encoding branching.
- ReadAll: unchanged (accumulates in `fd_states.buffer`, constructs
  `Value::bytes(data)` on scheduler heap).

### `src/io/aio.rs`

- `AsyncBackend::submit`: for Read/ReadLine, skip `buffer_pool.alloc()`.
  Extract buffer pointer via `writeable_buffer_ptr`.
- ReadLine/Read buffered-fast-path: copy buffered data into the
  pre-allocated buffer Value.
- `stdin_to_completion`: copy stdin thread data into the pre-allocated
  buffer.
- ReadAll: unchanged from current path.

### `src/io/threadpool.rs`

- `PoolOp::Read`, `PoolOp::ReadLine`: carry `FiberBuffer(*mut u8, usize)`
  instead of relying on BufferPool. The completion handler copies into the
  fiber's buffer via the raw pointer.
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
into the same InlineSlice — no copy, no `fd_states.buffer` intermediate.

### ReadAll (deferred)

ReadAll loops until EOF, accumulating in `fd_states.buffer`. The final Value
is constructed via `Value::bytes(data)` on the scheduler heap. This is the
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

1. `Value::string()` and `Value::bytes()` calls in `process_raw_completion`
   for Read/ReadLine operations.
2. Port encoding awareness in the I/O completion path.
3. Cross-heap pointers from scheduler to fiber for read completions.
4. Unbounded scheduler-heap accumulation from read-heavy fibers (Read/ReadLine).
5. `BufferPool` allocation for Read/ReadLine operations.
6. Intermediate buffers (`fd_states.buffer` as accumulator) for bounded reads.

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
7. **Encoding test:** Verify `port/read` returns bytes regardless of port
  encoding. Verify fiber-side UTF-8 conversion produces correct strings.
