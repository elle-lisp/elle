# primitives

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
- Define special forms (those are in `hir/analyze.rs`)
- Execute bytecode (that's `vm`)
- Compile code (that's `compiler`, `hir`, `lir`)

## Interface

| Function | Purpose |
|----------|---------|
| `register_primitives(vm, symbols)` | Install all primitives |
| `init_stdlib(vm, symbols)` | Load stdlib.lisp |

## Function type

**NativeFn**: `fn(&[Value]) -> (SignalBits, Value)`

All primitives use a single unified type. No primitive has VM access.
Return values:
- `(SIG_OK, value)` — success, push value onto stack
- `(SIG_ERROR, error_val(kind, msg))` — error, stored in `fiber.signal`
- `(SIG_RESUME, fiber_value)` — fiber resume, VM handles fiber swap
- `(SIG_QUERY, cons(keyword, arg))` — VM state query, dispatched by `dispatch_query()` in `signal.rs`

All SIG_RESUME primitives (including coroutine wrappers) return
`(SIG_RESUME, fiber_value)`. Fiber primitives (`fiber/resume`) return SIG_RESUME with the fiber value.
The VM swaps the child fiber into `vm.fiber`, executes it, then swaps back.
`emit` returns the signal bits directly — the VM's catch-all handler
stores them in `fiber.signal` and suspends the fiber.

## Adding a primitive

1. Create function in appropriate module
2. Register in that module's `register_*` function
3. That function is called by `registration.rs`

```rust
// In arithmetic.rs
pub fn prim_add(args: &[Value]) -> (SignalBits, Value) {
    // Implementation — return (SIG_ERROR, error_val("type-error", "msg")) for errors
}

pub fn register_arithmetic(vm: &mut VM, symbols: &mut SymbolTable) {
    let sym = symbols.intern("+");
    vm.set_global(sym.0, Value::native_fn(prim_add));
}
```

## Dependents

- `vm/call.rs` - dispatches primitive calls, handles signal bits
- `repl.rs` - registers primitives at startup
- `main.rs` - registers primitives at startup

## Invariants

1. **Primitives validate arguments.** Return `(SIG_ERROR, error_val(kind, msg))`
   for arity or type errors. Never panic.

2. **All primitives return `(SignalBits, Value)`.** No exceptions. Errors are
    signaled via SIG_ERROR with an error struct `{:error :keyword :message "message"}`.

3. **Most primitives have no VM access.** Operations that need the VM (fiber
   execution) return SIG_RESUME and let the VM dispatch loop handle it.
   Exceptions: primitives that read ambient VM state (`sys/args`, `ffi/native`,
   `import-file`, etc.) use `get_vm_context()` to access the VM as a
   read-only context. Do not use VM context for I/O or execution.

4. **Symbol table pointers are set before use.** The `length` primitive needs
   symbol table access to resolve symbol names. Call `set_length_symbol_table`
   before use, `clear_length_symbol_table` after. Keywords no longer need this
   — they carry their name directly via interned strings.

## Modules

| Module | Contains |
|--------|----------|
| `arithmetic.rs` | `+`, `-`, `*`, `/`, `mod`, `rem`, `abs`, `min`, `max`, `pow`, `sqrt`, `sin`, `cos`, `tan`, `log`, `exp`, `floor`, `ceil`, `round`, `even?`, `odd?`, `pi`, `e` |
| `comparison.rs` | `=` (numeric-aware), `identical?` (strict), `<`, `>`, `<=`, `>=` |
| `logic.rs` | `not` |
| `list.rs` | `cons`, `first`, `rest`, `list`, `length`, `empty?`, `append`, `concat`, `reverse`, `last`, `butlast`, `take`, `drop` |
| `array.rs` | `array`, `@array`, `array/new`, `push`, `pop`, `popn`, `insert`, `remove` |
| `string.rs` | `@string` (constructor), `string/upcase`, `string/downcase`, `string/slice`, `string/find`, `string/split`, `string/replace`, `string/trim`, `string/contains?`, `string/starts-with?`, `string/ends-with?`, `string/join`, `string/size-of` |
| `format.rs` | `string/format` |
| `table.rs` | `@struct`, `del`, `keys`, `values`, `has-key?` (imports `get`/`put` from `access.rs`) |
| `access.rs` | `get`, `put` — polymorphic collection access; @string `put` uses grapheme-cluster indexing (matching immutable `string`), value must be a string |
| `sets.rs` | `set`, `@set`, `set?`, `contains?`, `add`, `del`, `union`, `intersection`, `difference`, `set->array`, `seq->set` |
| `structs.rs` | `struct` |
| `fileio.rs` | `slurp`, `spit` |
| `path.rs` | `path/join`, `path/parent`, `path/filename`, `path/stem`, `path/extension`, `path/with-extension`, `path/normalize`, `path/absolute`, `path/canonicalize`, `path/relative`, `path/components`, `path/absolute?`, `path/relative?`, `path/cwd`, `path/exists?`, `path/file?`, `path/dir?` |
| `ports.rs` | `port/open`, `port/open-bytes`, `port/close`, `port/stdin`, `port/stdout`, `port/stderr`, `port?`, `port/open?`, `port/set-options` |
| `net.rs` | `tcp/listen`, `tcp/accept`, `tcp/connect`, `tcp/shutdown`, `udp/bind`, `udp/send-to`, `udp/recv-from` |
| `unix.rs` | `unix/listen`, `unix/accept`, `unix/connect`, `unix/shutdown` |
| `kwarg.rs` | `extract_keyword_timeout` helper function |
| `display.rs` | `print`, `println`, `display`, `newline` |
| `types.rs` | `nil?`, `pair?`, `list?`, `number?`, `integer?`, `float?`, `string?`, `boolean?`, `symbol?`, `keyword?`, `array?`, `struct?`, `bytes?`, `mutable?`, `type-of` |
| `cell.rs` | `box`, `unbox`, `rebox`, `box?` |
| `concurrency.rs` | `spawn`, `join`, `current-thread-id` |
| `chan.rs` | `chan/new`, `chan/send`, `chan/recv`, `chan/clone`, `chan/close`, `chan/close-recv`, `chan/select` |
| `coroutines.rs` | `coro/new`, `coro/resume`, `coro/done?`, `coro/status`, `coro/value`, `coro/>iterator` |
| `fibers.rs` | `fiber/new`, `fiber/resume`, `emit`, `fiber/status`, `fiber/value` |
| `fiber_introspect.rs` | `fiber/bits`, `fiber/mask`, `fiber/parent`, `fiber/child`, `fiber/propagate`, `fiber/cancel`, `fiber?` |
| `parameters.rs` | `make-parameter`, `parameter?` |
| `traits.rs` | `with-traits`, `traits` |
| `time.rs` | `clock/monotonic`, `clock/realtime`, `clock/cpu`, `time/sleep` |
| `time_def.rs` | `time/stopwatch`, `time/elapsed` (Elle definitions via `eval`) |
| `meta.rs` | `gensym`, `datum->syntax`, `syntax->datum`, `syntax-pair?`, `syntax-list?`, `syntax-symbol?`, `syntax-keyword?`, `syntax-nil?`, `syntax->list`, `syntax-first`, `syntax-rest`, `syntax-e`, `squelch`, `meta/origin` |
| `introspection.rs` | `closure?`, `jit?`, `silent?`, `coroutine?`, `fn/mutates-params?`, `fn/errors?`, `fn/arity`, `fn/captures`, `fn/bytecode-size`, `doc`, `vm/query`, `jit/rejections`, `keyword` (alias: `string->keyword`) |
| `disassembly.rs` | `fn/disasm`, `fn/disasm-jit`, `fn/flow`, `vm/list-primitives`, `vm/primitive-meta` |
| `arena.rs` | `arena/count`, `arena/stats`, `arena/set-object-limit`, `arena/object-limit`, `arena/bytes`, `arena/checkpoint`, `arena/reset`, `arena/allocs`, `arena/peak`, `arena/reset-peak`, `environment` |
| `debug.rs` | `debug/print`, `debug/trace`, `debug/memory` |
| `ffi.rs` | `resolve_type_desc`, `extract_pointer_addr` helpers; FFI tests |
| `loading.rs` | `ffi/native`, `ffi/lookup`, `ffi/signature`, `ffi/callback`, `ffi/callback-free` |
| `calling.rs` | `ffi/call` |
| `memory.rs` | `ffi/size`, `ffi/align`, `ffi/malloc`, `ffi/free`, `ffi/read`, `ffi/write`, `ffi/string`, `ffi/struct`, `ffi/array` |
| `subprocess.rs` | `exit`, `halt`, `sys/args` (returns args after the source file in argv, empty if none), `sys/env`, `subprocess/exec`, `subprocess/wait`, `subprocess/kill`, `subprocess/pid` |

## string/format primitive

**Location:** `src/primitives/format.rs`

**Signature:** `(string/format template [args...])` or `(string/format template :key val ...)`

**Purpose:** Format a template string with positional or named arguments, supporting format specifications for alignment, padding, and numeric bases.

### Modes

**Positional mode:** Arguments are substituted in order.
```lisp
(string/format "{} + {} = {}" 1 2 3)  #=> "1 + 2 = 3"
(string/format "Hello, {}!" "Alice")  #=> "Hello, Alice!"
```

**Named mode:** Arguments are keyword-value pairs, substituted by name.
```lisp
(string/format "{name} is {age}" :name "Alice" :age 30)  #=> "Alice is 30"
(string/format "{greeting}, {name}!" :greeting "Hello" :name "Bob")  #=> "Hello, Bob!"
```

Cannot mix positional and named in the same template — error if both `{}` and `{name}` appear.

### Format specifications

Syntax: `{[name][:spec]}` where spec is `[[fill]align][width][.precision][type]`.

**Alignment:** `<` (left), `>` (right), `^` (center). Default: right for numbers, left for strings.

**Fill character:** Any char before alignment. Default: space. Example: `{:*^10}` → center with `*` padding.

**Width:** Minimum field width. Example: `{:10}` → pad to 10 chars.

**Precision:** For floats, decimal places. For strings, max chars. Example: `{:.2f}` → 2 decimal places.

**Type:** `d` (decimal), `x` (hex lowercase), `X` (hex uppercase), `o` (octal), `b` (binary), `f` (float), `e` (scientific), `s` (string).

**Examples:**
- `{:.2f}` — float with 2 decimal places
- `{:>10}` — right-align to 10 chars
- `{:<10}` — left-align to 10 chars
- `{:^10}` — center to 10 chars
- `{:05d}` — zero-pad integer to 5 digits
- `{:x}` — hex lowercase
- `{:X}` — hex uppercase
- `{:o}` — octal
- `{:b}` — binary
- `{:e}` — scientific notation
- `{:*^10}` — center with `*` fill to 10 chars

### Brace escaping

`{{` → `{`, `}}` → `}`. Escaping is processed in literal segments (outside placeholders).

```lisp
(string/format "literal {{braces}}")  #=> "literal {braces}"
```

### Error cases

| Condition | Error kind | Message |
|-----------|-----------|---------|
| Template not string | `type-error` | `"string/format: template must be string, got {type}"` |
| Unmatched `{` | `format-error` | `"string/format: unmatched '{' in template"` |
| Unmatched `}` | `format-error` | `"string/format: unmatched '}' in template"` |
| Positional arg count mismatch | `format-error` | `"string/format: expected N arguments, got M"` |
| Mixed positional/named | `format-error` | `"string/format: cannot mix positional and named arguments"` |
| Odd keyword args | `format-error` | `"string/format: odd number of keyword arguments"` |
| Non-keyword in named position | `type-error` | `"string/format: expected keyword, got {type}"` |
| Missing named key | `format-error` | `"string/format: missing key '{name}'"` |
| Extra named key | `format-error` | `"string/format: unexpected key '{name}'"` |
| Invalid format spec | `format-error` | `"string/format: invalid format spec '{spec}'"` |
| Type mismatch in format | `format-error` | `"string/format: cannot format {type} with spec '{char}'"` |

### Implementation details

- **Template parsing:** `parse_placeholders()` extracts `{...}` placeholders, handling `{{` and `}}` escapes.
- **Format spec parsing:** `parse_format_spec()` parses alignment, fill, width, precision, and type.
- **Value formatting:** `format_value()` applies spec to value, `format_raw()` produces unpadded string, `apply_width_align()` adds padding.
- **Mode dispatch:** `format_positional()` for `{}` placeholders, `format_named()` for `{name}` placeholders.
- **Output building:** `build_output()` reconstructs template with formatted values, `unescape_into()` handles brace escaping.

### Invariants

1. **No mixing modes.** Positional and named placeholders cannot coexist in the same template.
2. **Arity enforcement.** Positional mode requires exactly as many args as placeholders. Named mode requires even args (key-value pairs).
3. **Type safety.** Format specs are validated against value types (e.g., `d` requires integer, `f` requires number).
4. **Brace escaping.** `{{` and `}}` are unescaped only in literal segments, not inside placeholders.

## string/size-of primitive

**Location:** `src/primitives/string.rs`

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
| Argument not string | `type-error` | `"string/size-of: expected string, got {type}"` |
| Wrong arity | `arity-error` | `"string/size-of: expected 1 argument, got N"` |

**Invariants:**

1. **Byte-level semantics.** Returns UTF-8 byte count, not character count. This is essential for HTTP headers and binary protocols.
2. **No mutation.** The operation is pure and does not modify the string.
3. **Consistent with UTF-8.** The result matches `(length (bytes s))` for the UTF-8 encoding of the string.

## Sys Primitives

**Location:** `src/primitives/subprocess.rs`

- `sys/args` — Returns user-provided command-line arguments as an immutable
  array of strings. Arguments are those that follow the source file (or `-` for
  stdin) in the process argv. Returns an empty array `[]` if no args follow the
  source file, or if running in REPL mode. Reads from `vm.user_args` via
  `get_vm_context()`. Signal: `Signal::silent()`. Arity: `Exact(0)`.
  - Example: `elle script.lisp foo bar` → `sys/args` returns `["foo" "bar"]`
  - Flags after source: `elle script.lisp -v foo` → `sys/args` returns `["-v" "foo"]`
  - No trailing args: `elle script.lisp` → `sys/args` returns `[]`

- `sys/env` — Returns the process environment as an immutable struct
  `{"KEY" "value" ...}` with string keys. Uses `std::env::vars_os()` with
  `filter_map` to skip non-UTF-8 entries. Returns empty struct `{}` if no
  env vars. With an optional string argument `(sys/env "NAME")`, looks up a
  single variable and returns its value as a string, or `nil` if not set.
  Signal: `Signal::silent()`. Arity: `Range(0, 1)`.

## Subprocess Primitives

**Location:** `src/primitives/subprocess.rs`

**Capability bit:** `SIG_EXEC` (bit 11) is a capability bit for fiber mask access control. Subprocess primitives emit `SIG_EXEC | SIG_IO | SIG_YIELD` so that fiber signal masks can selectively allow or deny subprocess operations independently of general I/O. The dispatch mechanism remains `SIG_IO`-based — the `SIG_EXEC` bit exists for access control granularity, not for routing.

**Primitives:**

- `subprocess/exec program args [opts]` — Spawns a subprocess. Returns `{:pid int :stdin port|nil :stdout port|nil :stderr port|nil :process <external:process>}`. Emits `SIG_EXEC | SIG_IO | SIG_YIELD`. Pipes are binary by default; text decoding is the caller's responsibility.
  - `program` (string): path to executable
  - `args` (list or array of strings): command-line arguments — accepts empty list `()`, cons list, immutable array `[...]`, or mutable array `@[...]`
  - `opts` (optional struct): configuration with keys `:env` (struct of env vars, default: inherit), `:cwd` (string, default: inherit), `:stdin` (keyword `:pipe`/`:inherit`/`:null`, default: `:pipe`), `:stdout` (keyword, default: `:pipe`), `:stderr` (keyword, default: `:pipe`)
  - Error cases: non-sequence `args` → `type-error "subprocess/exec: args must be list, array, or @array, got {type}"`; non-string element → `type-error "subprocess/exec: args element must be string, got {type}"`; improper list → `type-error "subprocess/exec: improper list ending in {type}"`
  - Note: `subprocess/system` gets sequence widening for free via pass-through — it calls `subprocess/exec` directly with the `args` argument unchanged.

- `subprocess/wait handle` — Waits for a subprocess to exit. Returns exit code as integer (0 = success). Emits `SIG_EXEC | SIG_IO | SIG_YIELD`. Accepts either a process handle (external) or an exec result struct (extracts `:process` key).

- `subprocess/kill handle [signal]` — Sends a signal to a subprocess synchronously. Returns `nil` on success. Emits `SIG_ERROR` only (no yield). Default signal is `SIGTERM` (15). Accepts either a process handle or an exec result struct.

- `subprocess/pid handle` — Extracts the OS process ID from a process handle or exec result struct. Returns integer PID. Emits `SIG_ERROR` only (no yield). Accepts either a process handle (external) or an exec result struct (extracts `:process` key).

**Handle extraction pattern:** `subprocess/wait`, `subprocess/kill`, and `subprocess/pid` all accept either:
1. A direct process handle (external with type name "process")
2. An exec result struct with a `:process` key containing the handle

This allows both `(subprocess/wait proc)` (where `proc` is the result of `subprocess/exec`) and `(subprocess/wait (get proc :process))` (extracting the handle directly).

**Pipe ports:** Ports returned by `subprocess/exec` are created with `PortKind::Pipe` and `Encoding::Binary`. Subprocess output is an arbitrary byte stream; text decoding is the caller's responsibility via `(string bytes-val)` or `port/lines`.

## Network Primitives

**Location:** `src/primitives/net.rs`

**TCP primitives:**
- `tcp/listen addr port` — synchronous, returns listener port. Binds to address:port with `SO_REUSEADDR`, listens with backlog 128.
- `tcp/accept listener` or `tcp/accept listener :timeout ms` — yields `SIG_IO`, accepts incoming connection, returns stream port.
- `tcp/connect addr port` or `tcp/connect addr port :timeout ms` — yields `SIG_IO`, connects to address:port, returns stream port.
- `tcp/shutdown port how` — yields `SIG_IO`, gracefully shuts down stream. `how` is keyword `:read`, `:write`, or `:read-write`.

**UDP primitives:**
- `udp/bind addr port` — synchronous, returns UDP socket port. Binds to address:port with `SO_REUSEADDR`.
- `udp/send-to socket data addr port` or `udp/send-to socket data addr port :timeout ms` — yields `SIG_IO`, sends datagram, returns bytes sent.
- `udp/recv-from socket count` or `udp/recv-from socket count :timeout ms` — yields `SIG_IO`, receives datagram, returns struct `{:data bytes :addr string :port int}`.

**Unix domain socket primitives:**
- `unix/listen path` — synchronous, returns listener port. Creates Unix socket at path (or abstract socket if path starts with `@`). Unlinks existing file before bind.
- `unix/accept listener` or `unix/accept listener :timeout ms` — yields `SIG_IO`, accepts incoming connection, returns stream port.
- `unix/connect path` or `unix/connect path :timeout ms` — yields `SIG_IO`, connects to Unix socket at path, returns stream port.
- `unix/shutdown port how` — yields `SIG_IO`, gracefully shuts down stream. `how` is keyword `:read`, `:write`, or `:read-write`.

**Timeout support:** All yielding network primitives accept optional `:timeout ms` keyword argument. Timeout is resolved at scheduler level: per-call timeout overrides port-level timeout (set via `port/set-options`).

## Keyword Argument Helper

**Location:** `src/primitives/kwarg.rs`

**Function:** `extract_keyword_timeout(args: &[Value], start: usize, prim_name: &str) -> Result<Option<Duration>, (SignalBits, Value)>`

Scans args starting at index `start` for keyword-value pairs. Currently recognizes `:timeout ms` where `ms` is a non-negative integer. Returns `Ok(None)` if `:timeout` is absent, `Ok(Some(duration))` if present, or `Err(...)` on bad keyword, missing value, or bad type.

Used by network primitives and stream primitives to parse optional timeout arguments.

## Port Options Primitive

**Location:** `src/primitives/ports.rs`

**Primitive:** `port/set-options port :timeout ms` (or `:timeout nil` to clear)

Sets port-level options. Currently supports `:timeout ms` (non-negative integer in milliseconds, or nil to clear). Stored as `Cell<Option<u64>>` on Port struct. Unknown keywords signal error. Odd trailing args signal error.

## squelch Primitive

**Location:** `src/primitives/meta.rs`

**Signature:** `(squelch closure :kw1 :kw2 ...)`

**Purpose:** Transform a closure by applying a runtime signal squelch mask. Returns a new closure that, when called, intercepts signals matching the keywords and converts them to `:error` with kind `"signal-violation"`.

**Behavior:**
- Takes a closure as the first argument
- Takes one or more signal keywords as remaining arguments
- Returns a **new** closure (same template and environment, new squelch mask)
- When the returned closure is called, if it emits a squelched signal, the signal is converted to `:error`
- Non-squelched signals pass through normally
- Errors are never affected by squelch (they pass through unchanged)
- Composable: `(squelch (squelch f :yield) :io)` squelches both `:yield` and `:io`

**Signal:** `Signal::errors()` (can error on bad arguments, otherwise silent)

**Arity:** `AtLeast(2)` — closure + at least one keyword

**Error cases:**

| Condition | Error kind | Message |
|-----------|-----------|---------|
| `(squelch f)` with no keywords | `arity-error` | `"squelch: expected at least 2 arguments (closure + keywords), got 1"` |
| `(squelch non-closure :yield)` | `type-error` | `"squelch: first argument must be a closure, got {type}"` |
| `(squelch f non-keyword)` | `type-error` | `"squelch: expected signal keyword, got {type}"` |
| Unknown keyword | `error` | `"squelch: signal :X not registered (unknown signal keyword)"` |

**Implementation details:**
- Validates first argument is a closure via `as_closure()`
- Validates remaining arguments are keywords via `as_keyword_name()`
- Looks up each keyword in the global signal registry via `registry::global_registry().lock().unwrap().lookup()`
- ORs bits into a combined mask
- Creates new closure with `squelch_mask = closure.squelch_mask | new_bits`
- Returns the new closure as a Value

**Tail-call enforcement:** Squelch enforcement works correctly on tail-call invocation (fixes issue #588). The `squelch_mask` is carried through the tail-call trampoline loop in `execute_bytecode_saving_stack` via the `TailCallInfo` struct. After each tail-call iteration, the mask is re-applied before the next callee executes.

## meta/origin Primitive

**Location:** `src/primitives/meta.rs`

**Signature:** `(meta/origin f)`

**Purpose:** Return the source location of a closure as `{:file :line :col}`, or `nil` if unavailable.

**Behavior:**
- If `f` is not a closure, returns `nil`
- If the closure has no stored `syntax` field, returns `nil`
- If the syntax span has no `file`, returns `nil`
- Otherwise returns `{:file "path" :line N :col N}` where `:file` is the path string, `:line` is 1-based line number, `:col` is 0-based column number

**Examples:**
```lisp
(defn foo () 42)
(meta/origin foo)
#=> {:col 0 :file "/path/to/script.lisp" :line 1}

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
3. **Line and col are integers.** `:line` is the 1-based line number; `:col` is the 0-based column offset within the line.
4. **Result is an immutable struct.** The returned value is a `{...}` struct, not a mutable `@{...}`.

## Stream Primitive Timeout Support

**Location:** `src/primitives/stream.rs`

All 5 stream primitives now accept optional `:timeout ms` keyword argument:
- `stream/read-line port` or `stream/read-line port :timeout ms`
- `stream/read port count` or `stream/read port count :timeout ms`
- `stream/read-all port` or `stream/read-all port :timeout ms`
- `stream/write port data` or `stream/write port data :timeout ms`
- `stream/flush port` or `stream/flush port :timeout ms`

Arity changed from `Exact(N)` to `AtLeast(N)` to allow keyword args. Timeout is extracted via `extract_keyword_timeout` and passed to `IoRequest::with_timeout()`.

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~60 | Re-exports |
| `registration.rs` | ~190 | `register_primitives`, `build_primitive_meta`, `cached_primitive_meta` |
| `module_init.rs` | ~170 | `init_stdlib`, module initialization |
| `introspection.rs` | ~384 | Function introspection predicates and metadata queries |
| `disassembly.rs` | ~416 | Bytecode/JIT disassembly and CFG extraction |
| `arena.rs` | ~577 | Heap arena management primitives |
| `debug.rs` | ~221 | Debug print, trace, memory usage |
| `ffi.rs` | ~340 | FFI type resolution helpers and tests |
| `loading.rs` | ~330 | FFI library loading, symbol lookup, signatures, callbacks |
| `calling.rs` | ~95 | FFI function call dispatch |
| `memory.rs` | ~530 | FFI memory management, typed access, type construction |
| `chan.rs` | varies | `chan/new`, `chan/send`, `chan/recv`, `chan/clone`, `chan/close`, `chan/close-recv`, `chan/select` |
| `format.rs` | ~525 | `string/format` entry point, template parsing, value formatting, mode dispatch |
| `formatspec.rs` | ~202 | `FormatSpec` type, `Align`, `FormatType`, `parse_format_spec`, `spec_type_char` |
| `net.rs` | ~683 | TCP and UDP primitives, shared helpers, PRIMITIVES array, tests |
| `unix.rs` | ~160 | Unix domain socket primitives |
| `access.rs` | ~634 | Polymorphic `get`/`put` for all collection types |
| `fiber_introspect.rs` | ~357 | Fiber introspection and management primitives, PRIMITIVES array |
| `kwarg.rs` | ~100 | `extract_keyword_timeout` helper, tests |
| (others) | varies | Individual primitive implementations |