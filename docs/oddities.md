# Intentional Oddities

These are design choices that look wrong but are intentional. They exist for good reasons — usually performance, simplicity, or consistency with other Lisps.

## Critical: These cause bugs if misunderstood

### nil vs empty list (HIGHEST PRIORITY — causes infinite loops)

`nil` and `()` are distinct values with different truthiness:
- `Value::NIL` is falsy (represents absence)
- `Value::EMPTY_LIST` is truthy (it's a list, just empty)

Lists are `EMPTY_LIST`-terminated, not `NIL`-terminated. `(rest (list 1))` returns `EMPTY_LIST`. Use `empty?` (not `nil?`) to check for end-of-list. `nil?` only matches `Value::NIL`. This distinction matters in recursive list functions and affects `demos/docgen/` and `examples/`. **Getting this wrong causes infinite recursion.**

### `#` is comment, `;` is splice

`#` is the comment character (not `;`). `;expr` is the splice operator (array-spreading). `true`/`false` are the boolean literals (not `#t`/`#f`).

### `assign` not `set` for mutation

`assign` is the form for variable mutation: `(assign var value)`. This is distinct from the `set` constructor primitive for creating set values. Agents reflexively write `(set x val)` — this creates a set, not a mutation.

### `silence` is a preamble declaration; `squelch` is a runtime closure transform

`silence` only appears inside lambda bodies as a preamble declaration (after optional docstring, before first non-declaration expression). It is NOT a general expression form. Using it outside a lambda body is a call to the stdlib function, which signals `:error` at runtime.

`squelch` is a **primitive function**, not a preamble declaration. `(squelch f :yield)` takes a closure and returns a **new** closure that catches `:yield` at runtime and converts it to `:error`. It can appear anywhere an expression is valid.

**Key distinction:** `silence` is a **compile-time total suppressor** — `(silence f)` means f must emit nothing at all (enforced at compile time and runtime). Signal keywords are not accepted; `(silence f :error)` is a compile error. `squelch` is a **runtime blacklist** (open-world) — `(squelch f :yield)` returns a new closure that forbids `:yield` at the call boundary, but may emit anything else including user-defined signals.

```janet
# Correct: silence in lambda body preamble (compile-time suppression)
(fn (f x)
  (silence f)  # f must be completely silent
  (f x))

# Correct: squelch as an expression (runtime transform)
(fn (f x)
  (let ((safe-f (squelch f :yield)))  # returns a new closure
    (safe-f x)))

# Also correct: squelch outside lambda body
(let ((safe-f (squelch f :yield)))
  (safe-f))

# Runtime error: silence outside lambda body
(silence f)  # Error: signals :error at runtime

# Arity error: squelch with no keywords
(squelch f)  # Error: expected at least 2 arguments, got 1

# Type error: squelch on non-closure
(squelch 42 :yield)  # Error: first argument must be a closure
```

### Collection literal mutable/immutable split

Bare delimiters are immutable, `@`-prefixed are mutable:
- `[...]` → array (immutable), `@[...]` → @array (mutable)
- `{...}` → struct (immutable), `@{...}` → @struct (mutable)
- `|...|` → set (immutable), `@|...|` → @set (mutable)
- `"..."` → string (immutable), `@"..."` → @string (mutable)

Bytes (immutable binary data) and @bytes (mutable binary data) have no reader literal syntax — they are constructed via primitives: `(bytes 1 2 3)`, `(@bytes 1 2 3)`, `(string->bytes "hello")`, `(string->blob "hello")`. Display format is `#bytes[hex ...]` and `#@bytes[hex ...]` (output-only, not readable).

In `match`, `[...]` matches arrays (`IsArray`), `@[...]` matches @arrays (`IsArrayMut`), `{...}` matches structs (`IsStruct`), `@{...}` matches @structs (`IsStructMut`), `|x|` matches sets (`IsSet`), `@|x|` matches @sets (`IsSetMut`). In destructuring (`def`/`let`/`fn`), no type guards — `ArrayRefOrNil`/`TableGetOrNil` handle both mutable and immutable types.

## Implementation details

### Two lbox types: LBox vs LocalLBox

Two lbox types exist: `LBox` (user-created via `box`, explicit) and `LocalLBox` (compiler-created for mutable captures, auto-unwrapped).

### Coroutine primitives as fiber wrappers

Coroutine primitives (`coro/resume`) are implemented as fiber wrappers. They return `(SIG_RESUME, fiber_value)` and the VM's SIG_RESUME handler in `vm/call.rs` performs the actual fiber execution. This avoids primitives needing VM access.

### Cons type in value/heap.rs

The `Cons` type in `value/heap.rs` is the heap-allocated cons cell data. `Value::cons(car, cdr)` creates a tagged-union pointer to a heap Cons.

### Signal bits partitioning

Signal bits are partitioned: Bits 0-2 are user-facing (error, yield, debug), Bits 3-10 are VM-internal (resume, FFI, propagate, abort, query, halt, IO, terminal), Bits 11-15 are reserved, and Bits 16-31 are for user-defined signal types. Bit 6 (`SIG_ABORT`) is a composite (`SIG_ERROR | SIG_TERMINAL`), not a standalone bit at position 6. Bit 10 (`SIG_TERMINAL`) marks signals as uncatchable — they pass through mask checks regardless. See `src/value/fiber.rs` for the constants and `src/signals/mod.rs` for the partitioning comment.

### Destructuring silent nil semantics

Destructuring uses **silent nil semantics**: missing values become `nil`, except `CdrOrNil` which returns `EMPTY_LIST` for non-cons inputs (the rest of an exhausted list is an empty list, not absence). Wrong types produce `nil`, no runtime errors. This is separate from `match` pattern matching which is conditional. `CarOrNil`/`CdrOrNil`/`ArrayRefOrNil`/`ArraySliceFrom`/`TableGetOrNil` are dedicated bytecode instructions for this — they never signal errors. `ArrayRefOrNil` and `ArraySliceFrom` handle both arrays and tuples — bracket destructuring works on any indexed sequential type. In `match`, however, `[a b]` patterns only match arrays (the `IsArray` guard rejects tuples before element extraction). In `match`, compound patterns (`Cons`, `List`, `Array`, `Table`) emit type guards (`IsPair`, `IsArray`, `IsTable`) that branch to the fail label before extracting elements.

### Prelude macros

`defn`, `let*`, `->`, `->>`, `when`, `unless`, `try`/`catch`, `protect`, `defer`, `with`, `yield*`, `case`, `if-let`, `when-let`, and `forever` are prelude macros defined in [`prelude.lisp`](../prelude.lisp) (project root), loaded by the Expander before user code expansion. The prelude is embedded via `include_str!` (in `src/syntax/expand/mod.rs`) and parsed/expanded on each Expander creation.

### Collection literals: detailed syntax mapping

Collection literals follow the mutable/immutable split (see `docs/types.md`): bare delimiters are immutable, `@`-prefixed are mutable. `{:key val ...}` → struct (immutable). `@{:key val}` → @struct (mutable). `[1 2 3]` → array (immutable). `@[1 2 3]` → @array (mutable). `"hello"` → string (immutable). `@"hello"` → @string (mutable). `|1 2 3|` → set (immutable). `@|1 2 3|` → mutable set. Bytes (immutable binary data) and @bytes (mutable binary data) have no reader literal syntax — they are constructed via primitives: `(bytes 1 2 3)`, `(@bytes 1 2 3)`, `(string->bytes "hello")`, `(string->blob "hello")`. Display format is `#bytes[hex ...]` and `#@bytes[hex ...]` (output-only, not readable). `SyntaxKind::Tuple` represents `[...]`, `SyntaxKind::Array` represents `@[...]`, `SyntaxKind::Struct` represents `{...}`, `SyntaxKind::Table` represents `@{...}`, `SyntaxKind::Set` represents `|...|`, `SyntaxKind::SetMut` represents `@|...|`. The reader produces all six directly (no desugaring to List with prepended symbols). `@"..."` desugars to `(thaw "...")`. In `match`, `[...]` matches arrays (`IsArray`), `@[...]` matches @arrays (`IsArrayMut`), `{...}` matches structs (`IsStruct`), `@{...}` matches @structs (`IsStructMut`), `|x|` matches sets (`IsSet`), `@|x|` matches mutable sets (`IsSetMut`). In destructuring (`def`/`let`/`fn`), no type guards — `ArrayRefOrNil`/`TableGetOrNil` handle both mutable and immutable types.

### `|` delimiter for set literals

`|...|` is the immutable set literal syntax; `@|...|` is the mutable set literal. `|` is a delimiter (like `(`, `[`, `{`). Inside lists, arrays, structs, and tables, a bare `|` starts a nested set literal (delegates to `read_set`), not a special marker node.

### `:@name` keyword syntax

`:@name` is valid keyword syntax. The lexer recognizes `:@` as a keyword prefix variant. The `@` is consumed and prepended to the keyword name. Examples: `:@set`, `:@array`, `:@string`. These are used for mutable type keywords returned by `(type-of x)` on mutable collections.

### `[...]` dual meaning in expression vs structural position

`[...]` has dual meaning depending on position. In expression position, it's an array literal (`SyntaxKind::Array`). In structural positions of special forms — lambda params, binding lists, binding pairs, cond clauses, match arms, defmacro params — it's accepted interchangeably with `(...)`. `@[...]` (mutable @array) is intentionally rejected in structural positions.

### `;expr` splice operator: detailed semantics

`;expr` is the splice reader macro (Janet-style). It marks a value for array-spreading at call sites and data constructors. `(splice expr)` is the long form. `;` is a delimiter, so `a;b` is three tokens. `,;` is unquote-splicing (inside quasiquote), not comma + splice. Splice works on @arrays, arrays, and lists. Structs and @structs reject splice at compile time (key-value semantics). When a call has spliced args, the lowerer builds an args array (`MakeArray` → `ArrayExtend`/`ArrayPush` → `CallArray`) instead of the normal `Call` instruction. Arity checking is disabled for spliced calls.

### Or-patterns use `(or ...)` syntax

`|` is a delimiter for set literals (`|1 2 3|` for immutable sets, `@|1 2 3|` for mutable sets). `|` always starts a set literal, including inside lists, arrays, structs, and tables (delegates to `read_set`). Or-patterns use `(or pat1 pat2 pat3)` syntax — the `or` symbol in pattern position is recognized by the match analyzer in `special.rs`.

### `begin` vs `block` distinction

`begin` and `block` are distinct forms. `begin` sequences expressions without creating a scope (bindings leak into the enclosing scope). `block` sequences expressions within a new lexical scope (bindings are contained). `block` supports an optional keyword name and `break` for early exit: `(block :name body...)` / `(break :name value)`. `break` is validated at compile time — it must be inside a block and cannot cross function boundaries.

### ExternalObject uses Rc<dyn Any>

`ExternalObject` uses `Rc<dyn Any>` despite the general preference for typed values. This is intentional — plugins are dynamically loaded and the core compiler cannot know their types at compile time. The `type_name` field provides Elle-side identity, and `downcast_ref` is used only within the plugin that created the type.

### Module convention

Module files (`.lisp`) follow a standard pattern. The last expression in a module is a closure that returns a struct of exports. This allows parameterized modules in the future. Example:

```lisp
# module defines functions...
(defn assert-eq [a b] ...)
(defn assert-true [x] ...)

# last expression is a closure returning exports
(fn [] {:assert-eq assert-eq :assert-true assert-true})
```

When imported via `import-file`, the module's last expression (a closure) is returned. Call it to get the exports struct:

```lisp
(def asserts ((import-file "assertions.lisp")))
(asserts :assert-eq 1 1)
```

Or destructure directly:

```lisp
(def {:assert-eq assert-eq :assert-true assert-true} ((import-file "assertions.lisp")))
```

The `import-file` primitive (Chunk 3) uses `eval_file` to compile and execute the module, returning its last expression's value. For `.so` plugins, it returns `true`.

### Docstring extraction

Docstrings are extracted from leading string literals in function bodies. `HirKind::Lambda` has a `doc: Option<Value>` field, threaded through LIR and into `Closure.doc`. The `(doc name)` primitive checks closure doc fields on globals before falling back to builtin docs. LSP hover shows user-defined docstrings and builtin docs via `vm.docs`.

### `parameterize` special form

`parameterize` is a special form that creates a dynamic binding frame. Unlike lexical bindings (`let`, `fn` params), parameters are looked up at runtime from a stack of frames. `(make-parameter default)` creates a parameter; calling it reads the current value. `(parameterize ((p1 v1) (p2 v2) ...) body ...)` pushes a frame, executes the body, then pops the frame. Child fibers inherit parent parameter frames. Parameters are useful for simulating I/O ports, configuration, and other dynamic context.

### `syntax-case` generates code, not expansion-time evaluation

`syntax-case` in Elle is NOT evaluated at macro expansion time. It is a code-generating transformation: the expander rewrites it into a chain of `if`/`let` forms using the syntax predicates (`syntax-pair?`, `syntax-list?`, etc.). The scrutinee must be a macro parameter — it has no value at expansion time. The generated code runs when the macro transformer closure executes inside the VM.
