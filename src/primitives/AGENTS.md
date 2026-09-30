# primitives

<!-- audited: 2026-09-30 -->

Built-in functions. Registered into the VM at startup.

## Responsibility

Implement Elle's standard library of built-in functions:
- Arithmetic, comparison, logic
- List and array operations
- String manipulation
- I/O and file operations
- Concurrency primitives
- FFI support functions
- Introspection and debugging

Does NOT:
- Define special forms (those are in
  [hir/analyze/](../hir/analyze/AGENTS.md)). Note: `emit` is a special form
  when the first argument is a literal keyword or keyword set; dynamic
  `(emit var val)` falls through to the primitive.
- Execute bytecode (that's `vm`)
- Compile code (that's `compiler`, `hir`, `lir`)

## Interface

| Function | Purpose |
|----------|---------|
| `register_primitives(vm, symbols)` | Install all primitives |
| `init_stdlib(vm, symbols, cctx, cache)` | Load stdlib.lisp |

## Function type

**NativeFn**: `&'static PrimitiveDef` (the bare function pointer type is `PrimFn`: `fn(&mut NativeCtx, &[Value]) -> (SignalBits, Value)`)

All primitives use a single unified type. Every primitive receives a
`&mut NativeCtx` and reaches the driving VM via `ctx.vm()`, its symbols via
`ctx.vm().symbols()`, and the heap via `ctx.heap_mut()`; values are allocated
through the ctx (`ctx.string(..)`, `ctx.pair(..)`, …) so each is born in the
call's own region.
Return values:
- `(SIG_OK, value)` — success
- `(SIG_ERROR, ctx.error(kind, msg))` — error
- `(SIG_RESUME, fiber_value)` — fiber context switch (see [vm/](../vm/AGENTS.md))
- `(SIG_QUERY, ctx.pair(keyword, arg))` — VM state query (see [vm/](../vm/AGENTS.md))

## Adding a primitive

[primitives.md](../../docs/cookbook/primitives.md) holds the recipe. Declare
the new primitive's `effect: RegionEffect::…` ([def.rs](def.rs); the spec is
[region effects](../../docs/impl/region/effects.md)). Every shipped table is fully declared —
do not leave a new primitive at the `Unknown` default. The claim is
checked forever by the declaration oracle (`dispatch_native_call`,
debug builds): Immediate = non-heap result; Fresh = heap result in
this call's own region; PassThrough = never fresh, no stores;
Stores{args} = uncounted store into a structure (containment), fresh
result; Sends{args} = the args cross a fiber boundary (`chan/send`'s
message), seam-counted at the send (`EscapeSite::ChanSend`) so no edges,
plus a fiber-frontier Shared seed
for the ownership forest; Funnel = every store rides the runtime-counted
mutable-store funnel; Delivers{args} = the args are installed in another
fiber's signal slot, counted by that seam; Opaque = stores nothing but the
result lives neither in this call's region nor in an argument's; Mixed =
examined, and the native (or the VM handler for the signal it returns)
stores an argument uncounted. The last two are the pair most often
confused: the arg clique and the store-facet escape seed are keyed on the
STORE, so a native that stores nothing declares Opaque however unbounded
its result.

## Dependents

- [vm/call.rs](../vm/call.rs) - dispatches primitive calls, handles signal bits
- [repl.rs](../repl.rs) - REPL session (form-by-form eval, def persistence)
- [runtime.rs](../runtime.rs) - registers primitives when an instance boots

## Invariants

1. **Primitives validate arguments.** The VM checks the declared arity before
   the call. Return `(SIG_ERROR, ctx.error(kind, msg))` for any other bad
   argument. Never panic.

2. **All primitives return `(SignalBits, Value)`.** No exceptions. Errors are
    signaled via SIG_ERROR with an error struct `{:error :keyword :message "message"}`.

3. **Primitives reach the VM through `ctx`.** Operations that drive fiber
   execution return SIG_RESUME and let the VM dispatch loop handle it.
   Primitives that read VM state (`sys/args`, `ffi/native`, `import-file`, etc.)
   reach it via `ctx.vm()`. Do not use `ctx.vm()` for I/O or interpreter
   re-entry that the dispatch loop owns.

4. **Symbol names resolve through the driving VM.** The `length` primitive
   resolves symbol names via `ctx.vm().symbols()`. A keyword's spelling
   resolves through the same memo, then the static vocabulary
   ([symbol.md](../../docs/impl/symbol.md)).

## Modules

Each row names a module's registered primitives by their canonical names.
Aliases are declared beside each definition and are not repeated here; `(doc
name)` answers for either spelling.

| Module | Registers |
|--------|-----------|
| [allocator.rs](allocator.rs) | `allocator/install`, `allocator/uninstall` |
| [arena.rs](arena.rs) | `debug/arena-stats`, `debug/arena-count`, `debug/arena-bytes`, `debug/arena-allocs`, `debug/arena-peak`, `debug/arena-region-of`, `debug/arena-dump`, `debug/arena-region-table`, and the rest of the `debug/arena-*` family |
| [arena/reclaim.rs](arena/reclaim.rs) | `debug/arena-region-frees`, `debug/arena-adopts`, `debug/arena-owned`, and the rest of the reclamation gauges ([diagnostics](../../docs/impl/region/diagnostics.md)) |
| [arithmetic.rs](arithmetic.rs) | nothing. `+`, `-`, `*` and their peers are stdlib closures over the `%`-intrinsics; the table here is empty and the file holds shared helpers |
| [array.rs](array.rs) | `array`, `@array`, `array/new`, `popn`, `insert`, `remove` |
| [bitwise.rs](bitwise.rs) | `bit/and`, `bit/or`, `bit/xor`, `bit/not`, `bit/shl`, `bit/shr` |
| [box.rs](box.rs) | `box`, `unbox`, `rebox` |
| [bytes.rs](bytes.rs) | `bytes`, `@bytes`, `seq->hex`, `slice` |
| [chan.rs](chan.rs) | `chan`, `chan/send`, `chan/recv`, `chan/clone`, `chan/close`, `chan/close-recv`, `chan/try-select`, `chan/wait-ready` (see "Channel select wake protocol" below for the `chan/select` Lisp wrapper) |
| [comparison.rs](comparison.rs) | `=` (numeric-aware), `identical?` (strict), `hash`. The ordering comparisons are stdlib closures over `%lt`/`%gt`/`%le`/`%ge` |
| [compile/](compile/mod.rs) | `compile/analyze`, `compile/symbols`, `compile/captures`, `compile/call-graph`, `compile/run-on`, `compile/whole-module`, and the rest of the `compile/*` family |
| [config.rs](config.rs) | `vm/tier`, `backend?`, `vm/config`, `vm/config-set` |
| [concurrency.rs](concurrency.rs) | `sys/spawn`, `sys/spawn-vm`, `sys/thread-state`, `sys/thread-id`, `sys/unique`; the spawn worker body is [concurrency/worker.rs](concurrency/worker.rs) |
| [convert.rs](convert.rs) | `integer`, `float`, `parse-int`, `parse-float`, `string`, `number->string` |
| [debug.rs](debug.rs) | `debug/print`, `debug/trace`, `debug/memory`, `debug/symbol-count` |
| [disassembly.rs](disassembly.rs) | `fn/disasm`, `fn/disasm-jit`, `fn/flow`, `vm/list-primitives`, `vm/primitive-meta` |
| [display.rs](display.rs) | `pp`, `describe`. The output verbs (`print`, `println`, `eprint`, `eprintln`) are stdlib closures over the ports |
| [fiber_introspect.rs](fiber_introspect.rs) | `fiber/bits`, `fiber/mask`, `fiber/cancel`, `fiber/child`, `fiber/parent`, `fiber/propagate`, `fiber/caps`, `fiber/abort`, `fiber/refuse` |
| [fibers.rs](fibers.rs) | `fiber/new`, `fiber/resume`, `fiber/emit`, `fiber/status`, `fiber/value`, `fiber/set-fuel`, `fiber/fuel`, `fiber/clear-fuel` |
| [fileio.rs](fileio.rs) | `file/read`, `file/write`, `file/append`, `file/delete`, `file/delete-dir`, `file/delete-dir-all`, `file/mkdir`, `file/mkdir-all`, `file/mktempdir`, `file/rename`, `file/copy`, `file/size`, `file/ls`, `file/lines`, `file/stat`, `file/lstat` |
| [format.rs](format.rs) | `string/format` — see [format/](format/AGENTS.md) |
| [intrinsics.rs](intrinsics.rs) | the `%`-intrinsics: `%add`, `%get`, `%put`, `%has?`, `%first`, `%pop` and the rest. See [intrinsics](../../docs/intrinsics.md) |
| [introspection.rs](introspection.rs) | `jit?`, `silent?`, `fiber?`, `fn/arity`, `fn/captures`, `fn/errors?`, `fn/bytecode-size`, `fn/gpu-eligible?`, `doc`, `vm/query`, `signals`, `jit/rejections`, `keyword` |
| [io.rs](io.rs) | `io-request?`, `io-backend?`, `io/backend`, `io/submit`, `io/workers`, `io/reap`, `io/wait`, `io/cancel`, `ev/sleep`, `ev/poll-fd` |
| [json/](json/AGENTS.md) | `json/parse`, `json/serialize`, `json/pretty` |
| [list/](list/mod.rs) | `first`, `second`, `rest`, `list`, `length`, `empty?`, `->array`, `->list` |
| [loading.rs](loading.rs) | `ffi/native`, `ffi/lookup`, `ffi/on-unload`, `ffi/run-teardowns`, `ffi/signature`, `ffi/call`, `ffi/callback`, `ffi/callback-free` |
| [logic.rs](logic.rs) | `and`, `or` |
| [lstruct.rs](lstruct.rs) | `@struct`, `get`, `keys`, `values`, `has?` (the `get` body lives in [access.rs](access.rs)) |
| [math.rs](math.rs) | `math/sqrt`, `math/sin`, `math/cos`, `math/tan`, `math/log`, `math/exp`, `math/pow`, `math/atan2`, `math/pi`, `math/e`, `math/inf`, `math/nan`, and the rest of the `math/*` family |
| [memory.rs](memory.rs) | `ffi/size`, `ffi/align`, `ffi/malloc`, `ffi/free`, `ffi/read`, `ffi/write`, `ffi/string`, `ffi/struct`, `ffi/array`, `ptr/add`, `ptr/diff`, `ptr/to-int`, `ptr/from-int` |
| [meta.rs](meta.rs) | `meta/gensym`, `meta/datum->syntax`, `meta/syntax->datum`, the `meta/syntax-*` predicates, `meta/origin`, `squelch`, `attune`, `git`, `fn/git?`, `disgit` |
| [modules.rs](modules.rs) | `import` |
| [net.rs](net.rs) | `tcp/listen`, `tcp/accept`, `tcp/connect-ip`, `tcp/shutdown`, `udp/bind`, `udp/send-to`, `udp/recv-from`, `sys/resolve`, `sys/ip?` (`tcp/connect` is a stdlib wrapper over `tcp/connect-ip`) |
| [package.rs](package.rs) | `elle/version`, `elle/epoch`, `elle/build-profile`, `elle/executable`, `elle/root`, `elle/boot-fingerprint`, `elle/info` |
| [parameters.rs](parameters.rs) | `parameter` |
| [path.rs](path.rs) | `path/join`, `path/parent`, `path/filename`, `path/stem`, `path/extension`, `path/with-extension`, `path/normalize`, `path/absolute`, `path/canonicalize`, `path/relative`, `path/components`, `path/absolute?`, `path/relative?`, `path/cwd`, `path/exists?`, `path/file?`, `path/dir?` |
| [ports.rs](ports.rs) | `port/open`, `port/open-bytes`, `port/close`, `port/stdin`, `port/stdout`, `port/stderr`, `port?`, `port/open?`, `port/set-options`, `port/encoding`, `port/path`, `port/seek`, `port/tell` |
| [posix.rs](posix.rs) | `os/sig-send`, `os/sig-raise`, `os/sig-watch`, `os/sig-next`, `os/sig-close`, `os/sig-pending`, `os/sig-mask`, `os/sig-watching` — POSIX signal send and receive. Send and raise are gated on the `:os-signal` capability (`SIG_OS_SIGNAL`); the receive primitives are async and yield `:io`. See [posix signals](../../docs/posix-signals.md) |
| [read.rs](read.rs) | `read`, `read-all` |
| [sets.rs](sets.rs) | `set`, `@set`, `union`, `intersection`, `difference`, `seq->set`, `string-contains?` |
| [sort.rs](sort.rs) | `sort` |
| [stream.rs](stream.rs) | `port/read-line`, `port/read`, `port/read-exact`, `port/read-all`, `port/write`, `port/flush` |
| [string.rs](string.rs) | `@string`, `string/uppercase`, `string/lowercase`, `string/find`, `string/split`, `string/replace`, `string/trim`, `string/contains?`, `string/starts-with?`, `string/ends-with?`, `string/join`, `string/repeat`, `string/size-of`, `uri-encode` |
| [structs.rs](structs.rs) | `struct`, `freeze`, `deep-freeze`, `thaw`, `pairs` |
| [subprocess.rs](subprocess.rs) | `sys/exit`, `sys/trap-exit!`, `sys/halt`, `sys/args`, `sys/argv`, `sys/pid`, `sys/env`, and the `subprocess/*` table — see [subprocess/](subprocess/AGENTS.md) |
| [time.rs](time.rs) | `clock/monotonic`, `clock/realtime`, `clock/cpu`, `time/sleep` |
| [traits.rs](traits.rs) | `with-traits`, `traits`, `trait/method`, `trait/op`, `trait/iterable?` |
| [types.rs](types.rs) | `type-of`, `ptr?`, `callable?` |
| [unix.rs](unix.rs) | `unix/listen`, `unix/accept`, `unix/connect`, `unix/shutdown` |
| [watch.rs](watch.rs) | `watch`, `watch-add`, `watch-remove`, `watch-next`, `watch-close` |

[access.rs](access.rs) registers nothing of its own. It holds the polymorphic
`get` and `put` bodies that [lstruct.rs](lstruct.rs) and
[intrinsics.rs](intrinsics.rs) both call, so the two tiers cannot drift.
`get`, `keys`, `values` and `has?` also read a `subprocess`'s closed key set;
`put` and `del` refuse one.


## string/format primitive

Template parsing, the format specification grammar, and the positional and
named substitution modes: [format/](format/AGENTS.md).

## string/size-of primitive

**Location:** [string.rs](string.rs)

**Signature:** `(string/size-of s)`

**Purpose:** Returns the byte length of string `s` in UTF-8 encoding (not character count). Used for accurate `Content-Length` headers and other byte-level operations.

**Behavior:**
- Accepts a single string argument
- Returns an integer representing the number of bytes in the UTF-8 encoding
- For ASCII strings, byte length equals character count
- For multi-byte UTF-8 characters, byte length > character count

**Examples:**
```lisp
(string/size-of "hello")           #=> 5
(string/size-of "café")            #=> 5 (é is 2 bytes in UTF-8)
(string/size-of "🎉")              #=> 4 (emoji is 4 bytes in UTF-8)
(string/size-of "")                #=> 0
```

**Error cases:**

| Condition | Error kind | Message |
|-----------|-----------|---------|
| Argument not string | `type-error` | `"string/size-of: expected string or @string, got {type}"` |
| Wrong arity | `arity-error` | `"string/size-of: expected 1 argument(s), got N"` |

**Invariants:**

1. **Byte-level semantics.** Returns UTF-8 byte count, not character count. This is essential for HTTP headers and binary protocols.
2. **No mutation.** The operation is pure and does not modify the string.
3. **Consistent with UTF-8.** The result matches `(length (bytes s))` for the UTF-8 encoding of the string.

## Sys Primitives

**Location:** [subprocess.rs](subprocess.rs)

- `sys/args` — Returns user-provided command-line arguments as an immutable
  list of strings: the arguments that follow the source file, `-` or `--`, or
  a subcommand's arguments. Returns an empty list `()` if none follow, or in
  REPL mode. Reads `ctx.vm().user_args`.
  Signal: `Signal::silent()`. Arity: `Exact(0)`.
  - Example: `elle script.lisp foo bar` → `sys/args` returns `("foo" "bar")`
  - Flags after source: `elle script.lisp -v foo` → `sys/args` returns `("-v" "foo")`
  - No trailing args: `elle script.lisp` → `sys/args` returns `()`

- `sys/env` — Returns the process environment as an immutable struct
  `{"KEY" "value" ...}` with string keys. Uses `std::env::vars_os()` with
  `filter_map` to skip non-UTF-8 entries. Returns empty struct `{}` if no
  env vars. With an optional string argument `(sys/env "NAME")`, looks up a
  single variable and returns its value as a string, or `nil` if not set.
  Signal: `Signal::silent()`. Arity: `Range(0, 1)`.

## Subprocess Primitives

`subprocess/exec`, `subprocess/wait`, `subprocess/kill`, `subprocess/pid`,
`subprocess/exit`, `subprocess/rusage` and `subprocess?`, with the `subprocess` value they all take
and the one boundary that checks it: [subprocess/](subprocess/AGENTS.md).

**Pipe ports:** Ports a subprocess carries are created with `PortKind::Pipe` and `Encoding::Binary`. Subprocess output is an arbitrary byte stream; text decoding is the caller's responsibility via `(string bytes-val)` or `port/lines`.

## Network Primitives

**Location:** [net.rs](net.rs), and [unix.rs](unix.rs) for the Unix domain sockets

**TCP primitives:**
- `tcp/listen addr port` — synchronous, returns listener port. Binds to address:port with `SO_REUSEADDR`, listens with backlog 128.
- `tcp/accept listener` — yields `SIG_IO`, accepts incoming connection, returns stream port.
- `tcp/connect-ip ip port` — yields `SIG_IO`, connects to a **parsed IP literal**:port, returns stream port. A hostname is rejected synchronously. `tcp/connect` (the hostname-accepting public API) is a stdlib wrapper that resolves via `sys/resolve` then calls this per address.
- `tcp/shutdown port how` — yields `SIG_IO`, gracefully shuts down stream. `how` is keyword `:read`, `:write`, or `:read-write`.

**UDP primitives:**
- `udp/bind addr port` — synchronous, returns UDP socket port. Binds to address:port with `SO_REUSEADDR`.
- `udp/send-to socket data addr port` — yields `SIG_IO`, sends datagram, returns bytes sent.
- `udp/recv-from socket count` — yields `SIG_IO`, receives datagram, returns struct `{:data bytes :addr string :port int}`.

**Unix domain socket primitives:**
- `unix/listen path` — synchronous, returns listener port. Creates Unix socket at path (or abstract socket if path starts with `@`). Unlinks existing file before bind.
- `unix/accept listener` — yields `SIG_IO`, accepts incoming connection, returns stream port.
- `unix/connect path` — yields `SIG_IO`, connects to Unix socket at path, returns stream port.
- `unix/shutdown port how` — yields `SIG_IO`, gracefully shuts down stream. `how` is keyword `:read`, `:write`, or `:read-write`.

**Bounds:** every yielding network primitive takes `:timeout` and `:deadline`
([I/O deadlines](../../docs/io/timeout.md)).

## Keyword Argument Helper

**Location:** [kwarg.rs](kwarg.rs)

The one parser of the bounds and the connect options, so every I/O primitive
reads `:timeout` and `:deadline` alike.

## Port Options Primitive

**Location:** [ports/query.rs](ports/query.rs)

`port/set-options port :timeout s` gives the port a `:timeout` of its own, which
every call on the port that names none takes; `:timeout nil` removes it
([I/O deadlines](../../docs/io/timeout.md)).

## port/seek and port/tell Primitives

**Location:** [ports/query.rs](ports/query.rs)

### port/seek

**Signature:** `(port/seek port offset)` or `(port/seek port offset :from :start|:current|:end)`

**Purpose:** Seek to a byte offset in a file port. Returns the new absolute byte offset as int. Discards the per-fd read buffer before seeking to prevent stale buffered data from diverging from the kernel position.

**Behavior:**
- Takes 2 to 4 arguments; the VM refuses any other count, and 3 is an error
- Validates port is a file port (`PortKind::File`); errors on stdio or network ports
- Validates offset is an integer
- Parses optional `:from :start|:current|:end` pair; default is `:start` (SEEK_SET)
- Returns `SIG_IO` with an `IoRequest` containing `IoOp::Seek { offset, whence }`

**Error cases:**

| Condition | Error kind | Message |
|-----------|-----------|---------|
| 0, 1, or 5+ args | `arity-error` | `"port/seek: expected 2-4 argument(s), got N"` |
| 3 args (incomplete :from pair) | `arity-error` | `"port/seek: :from keyword requires a value"` |
| First arg not a port | `type-error` | `"port/seek: expected port, got {type}"` |
| Port is not a file port | `type-error` | `"port/seek: expected file port, got {kind}"` |
| Offset not an integer | `type-error` | `"port/seek: expected integer for offset, got {type}"` |
| args[2] not the keyword `:from` | `value-error` | `"port/seek: unknown keyword :{other}, expected :from"` |
| args[2] not a keyword at all | `type-error` | `"port/seek: expected keyword for third argument, got {type}"` |
| args[3] not `:start`/`:current`/`:end` keyword | `value-error` | `"port/seek: invalid :from value :{other}, expected :start, :current, or :end"` |
| args[3] not a keyword | `type-error` | `"port/seek: expected keyword for :from value, got {type}"` |

**Invariants:**

1. **Buffer discard on seek.** The scheduler/backend must discard any buffered read data after seek so that subsequent reads start from the new position.
2. **Default origin is SEEK_SET.** Omitting `:from` seeks from the start of the file.
3. **File ports only.** Non-file ports (stdin, stdout, stderr, TCP streams, etc.) always return type-error.

### port/tell

**Signature:** `(port/tell port)`

**Purpose:** Return the current logical read position in a file port. Logical position = kernel file offset minus buffered-but-unconsumed bytes.

**Behavior:**
- Takes exactly 1 argument
- Validates port is a file port; errors on other kinds
- Returns `SIG_IO` with an `IoRequest` containing `IoOp::Tell`

**Error cases:**

| Condition | Error kind | Message |
|-----------|-----------|---------|
| Wrong arity | `arity-error` | `"port/tell: expected 1 argument(s), got N"` |
| Argument not a port | `type-error` | `"port/tell: expected port, got {type}"` |
| Port is not a file port | `type-error` | `"port/tell: expected file port, got {kind}"` |

**Invariants:**

1. **Logical position.** The returned offset reflects the user-visible read position, not the raw kernel offset. The backend subtracts any buffered bytes from the kernel offset.
2. **Coherent with seek.** `(port/seek p N)` followed by `(port/tell p)` returns `N` (assuming no buffered bytes after seek).
3. **File ports only.** Non-file ports always return type-error.

## squelch Primitive

**Location:** [meta/syntaxops.rs](meta/syntaxops.rs)

**Signature:** `(squelch closure :keyword)` or `(squelch closure |:kw1 :kw2|)`

**Purpose:** Transform a closure by applying a runtime signal squelch mask.
Returns a new closure that, when called, intercepts the named signal(s) and
converts them to `:error` with kind `"signal-violation"`.

**Behavior:**
- Takes a closure and a signal spec: a keyword, set, array, list, or integer
  of signal bits, resolved by `resolve_signal_bits`
- Returns a **new** closure (same template and environment), whose squelch
  mask is the old mask OR the new bits
- `(squelch f |:yield :io|)` squelches both yield and io

**Signal:** `Signal::errors()`

**Arity:** `Exact(2)`

**Tail-call enforcement:** A squelch holds across a tail call. Each
`TailCallInfo` carries its callee's `squelch_mask`; `trampoline_loop` ORs it
into the activation's mask, and `end_activation` enforces the result when the
activation ends.

## meta/origin Primitive

**Location:** [meta/syntaxops.rs](meta/syntaxops.rs)

**Signature:** `(meta/origin f)`

**Purpose:** Return the source location of a closure as `{:file :line :col}`, or `nil` if unavailable.

**Behavior:**
- If `f` is not a closure, returns `nil`
- If the closure's template records no origin span, returns `nil`
- If the syntax span has no `file`, returns `nil`
- Otherwise returns `{:file "path" :line N :col N}` where `:file` is the path string, `:line` is the 1-based line number, `:col` is the 1-based column number

**Examples:**
```lisp
(defn foo () 42)
(meta/origin foo)
#=> {:col 1 :file "/path/to/script.lisp" :line 1}

(meta/origin 42)
#=> nil

(meta/origin nil)
#=> nil
```

**Signal:** `Signal::silent()` — never errors, returns `nil` for non-closures

**Arity:** `Exact(1)`

**Invariants:**

1. **Always returns or nil.** Never errors. Non-closures and closures without file info return `nil`.
2. **File path is the canonical string from the span.** It matches the path passed to the compiler, which is set by the reader when parsing a named file.
3. **Line and col are integers.** `:line` is the 1-based line number; `:col` is the 1-based column within the line.
4. **Result is an immutable struct.** The returned value is a `{...}` struct, not a mutable `@{...}`.

## Channel select wake protocol

**Location:** [chan.rs](chan.rs), [chan/prims.rs](chan/prims.rs) and
[chan/wake.rs](chan/wake.rs), with the public wrapper in [stdlib.lisp](../stdlib.lisp). The
header of [chan/wake.rs](chan/wake.rs) says why a select parks on a wake fd
rather than on crossbeam's blocking select. The doc comments on `WakeList` and
`ChanSelectGuard` there say how each fd is registered, signalled and closed.

Three primitives back the Lisp `chan/select`:

- `chan/try-select rxs` — non-blocking `Select::try_select`. Returns
  `[i v]`, `[:empty]`, or `[:disconnected]`. Errors if any receiver was
  explicitly closed via `chan/close-recv`.
- `chan/wait-ready rxs &named timeout deadline` — yielding park. Allocates the
  wake fd, registers, does a post-register `try_select` to close the
  cross-thread race between the wrapper's first `chan/try-select` and
  the register. Returns `[:ready i v]` (post-register fast hit, no
  yield), `[:disconnected]`, or returns `SIG_IO` carrying an
  `IoOp::ChanSelectPark(ChanSelectGuardCell)`. The request's bound
  flows through to a linked `LinkTimeout` SQE on uring or to the
  thread-pool `poll(2)` timeout.
- `chan/select` (Lisp wrapper in [stdlib.lisp](../stdlib.lisp)) — runs `chan/try-select`
  for the fast path, then loops: answer `[:timeout]` once the clock has
  reached the deadline, else call `chan/wait-ready` with it, match its result
  (`:ready` → return `[i v]`; `:disconnected` → return `[:disconnected]`;
  nil → `chan/try-select` and re-park on `:empty`).

Cancellation: if the fiber is aborted while parked, the scheduler
removes the `PendingOp::ChanSelectPark` entry, which drops the guard,
which closes the fd and deregisters. No leak.

`submit_uring_poll_add` takes an `Option<Duration>` and emits a linked
`LinkTimeout` SQE for it, which is what carries `ev/poll-fd`'s timeout on
uring.

## Stream Primitive Bounds

**Location:** [stream.rs](stream.rs)

Every stream primitive takes `:timeout` and `:deadline`
([I/O deadlines](../../docs/io/timeout.md)), and declares `AtLeast(N)` arity to
admit them.
