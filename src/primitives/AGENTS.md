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
`fiber/signal` returns the signal bits directly — the VM's catch-all handler
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
   signaled via SIG_ERROR with an error tuple `[:keyword "message"]`.

3. **No primitive has VM access.** Operations that need the VM (fiber
   execution) return SIG_RESUME and let the VM dispatch loop handle it.

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
| `array.rs` | `tuple`, `array`, `array/new`, `push`, `pop`, `popn`, `insert`, `remove` |
| `buffer.rs` | `buffer`, `string->buffer`, `buffer->string` |
| `string.rs` | `string/upcase`, `string/downcase`, `string/slice`, `string/find`, `string/char-at`, `string/split`, `string/replace`, `string/trim`, `string/contains?`, `string/starts-with?`, `string/ends-with?`, `string/join` |
| `format.rs` | `string/format` |
| `table.rs` | `table`, `get`, `put`, `del`, `keys`, `values`, `has-key?` |
| `structs.rs` | `struct` |
| `fileio.rs` | `slurp`, `spit` |
| `path.rs` | `path/join`, `path/parent`, `path/filename`, `path/stem`, `path/extension`, `path/with-extension`, `path/normalize`, `path/absolute`, `path/canonicalize`, `path/relative`, `path/components`, `path/absolute?`, `path/relative?`, `path/cwd`, `path/exists?`, `path/file?`, `path/dir?` |
| `ports.rs` | `port/open`, `port/open-bytes`, `port/close`, `port/stdin`, `port/stdout`, `port/stderr`, `port?`, `port/open?` |
| `display.rs` | `print`, `println`, `display`, `newline` |
| `types.rs` | `nil?`, `pair?`, `list?`, `number?`, `integer?`, `float?`, `string?`, `boolean?`, `symbol?`, `keyword?`, `array?`, `tuple?`, `table?`, `struct?`, `buffer?`, `box?`, `bytes?`, `blob?`, `type-of` |
| `concurrency.rs` | `spawn`, `join`, `current-thread-id` |
| `chan.rs` | `chan/new`, `chan/send`, `chan/recv`, `chan/clone`, `chan/close`, `chan/close-recv`, `chan/select` |
| `coroutines.rs` | `coro/new`, `coro/resume`, `coro/done?`, `coro/status`, `coro/value`, `coro/>iterator` |
| `fibers.rs` | `fiber/new`, `fiber/resume`, `fiber/signal`, `fiber/status`, `fiber/value`, `fiber/bits`, `fiber/mask`, `fiber/parent`, `fiber/child`, `fiber/propagate`, `fiber/cancel`, `fiber?` |
| `parameters.rs` | `make-parameter`, `parameter?` |
| `time.rs` | `clock/monotonic`, `clock/realtime`, `clock/cpu`, `time/sleep` |
| `time_def.rs` | `time/stopwatch`, `time/elapsed` (Elle definitions via `eval`) |
| `meta.rs` | `gensym`, `datum->syntax`, `syntax->datum` |
| `debugging.rs` | `closure?`, `jit?`, `pure?`, `coro?`, `fn/mutates-params?`, `fn/errors?`, `fn/arity`, `captures`, `bytecode-size`, `call-count`, `doc`, `global?`, `string->keyword`, `disbit`, `disjit`, `vm/list-primitives`, `vm/primitive-meta` |
| `debug.rs` | `debug-print`, `trace`, `memory-usage`, `arena/count`, `arena/stats`, `arena/scope-stats`, `environment` |
| `process.rs` | `exit`, `halt` |

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

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | 35 | Re-exports |
| `registration.rs` | ~1390 | `register_primitives`, `register_fn` |
| `module_init.rs` | ~170 | `init_stdlib`, module initialization |
| `chan.rs` | varies | `chan/new`, `chan/send`, `chan/recv`, `chan/clone`, `chan/close`, `chan/close-recv`, `chan/select` |
| `format.rs` | ~967 | `string/format` with positional/named modes, format specs, brace escaping |
| (others) | varies | Individual primitive implementations |
