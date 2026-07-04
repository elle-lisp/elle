# Intrinsics

Intrinsics are silent bytecode operations prefixed with `%`. They compile
directly to VM instructions with no validation, no signal emission, and
no rest-arg allocation.

## When to use intrinsics

Use intrinsics when:
- Writing **hot loops** where rest-arg allocation from variadic stdlib
  wrappers would inflate arena counts or slow execution.
- Writing code that must be **GPU-eligible** (`%`-intrinsics lower to
  SPIR-V/MLIR; stdlib wrappers do not).
- Writing code inside **silence/muffle** contexts where the stdlib
  wrappers' `:error` signal would cause a signal violation.
- Writing **allocation-sensitive tests** (arena/resource measurements)
  where stdlib call overhead must be excluded.

Use stdlib wrappers (`+`, `-`, `*`, etc.) in all other code. They validate
inputs, produce clear error messages, and handle mixed int/float promotion.

## Integer overflow

Integers are 64-bit two's-complement and **wrap on overflow**, in every
mode and on every tier:

```lisp
(+ 9223372036854775807 1)   # => -9223372036854775808
```

This is a consequence of the intrinsic design, not an accident. `%add` is
signal-free, and the compiler emits it for `+` whenever both operands are
proven ints — but a type proof cannot exclude overflow, so if overflow were
an error the specialization would change observable behavior. Wrapping is
the one semantics the unchecked instruction, the JIT, the GPU tiers, and
`--checked-intrinsics` (which validates *types*, not ranges) can all agree
on. Code that needs overflow detection should check its own ranges (see
`abs` in stdlib.lisp for the pattern).

Integer division and remainder by zero are errors in the stdlib wrappers;
`(/ i64-min -1)` and `(rem i64-min -1)` wrap (to `i64-min` and `0`).

## Complete list

### Arithmetic (2 args)

| Intrinsic | Args | Description |
|-----------|------|-------------|
| `%add`    | 2    | Addition |
| `%sub`    | 1-2  | Subtraction; `(%sub x)` negates |
| `%mul`    | 2    | Multiplication |
| `%div`    | 2    | Division |
| `%rem`    | 2    | Remainder (sign follows dividend) |
| `%mod`    | 2    | Modulo (sign follows divisor) |

### Comparison (2 args)

| Intrinsic | Args | Description |
|-----------|------|-------------|
| `%eq`     | 2    | Equality |
| `%lt`     | 2    | Less than |
| `%gt`     | 2    | Greater than |
| `%le`     | 2    | Less than or equal |
| `%ge`     | 2    | Greater than or equal |

### Logic (1 arg)

| Intrinsic | Args | Description |
|-----------|------|-------------|
| `%not`    | 1    | Logical not |

### Conversion (1 arg)

| Intrinsic | Args | Description |
|-----------|------|-------------|
| `%int`    | 1    | Convert to integer (truncates floats) |
| `%float`  | 1    | Convert to float |

### Data (1-2 args)

| Intrinsic | Args | Description |
|-----------|------|-------------|
| `%pair`   | 2    | Construct a pair cell |
| `%first`  | 1    | First element of a pair |
| `%rest`   | 1    | Rest of a pair |

### Bitwise (1-2 args)

| Intrinsic | Args | Description |
|-----------|------|-------------|
| `%bit-and`| 2    | Bitwise AND |
| `%bit-or` | 2    | Bitwise OR |
| `%bit-xor`| 2    | Bitwise XOR |
| `%bit-not`| 1    | Bitwise complement |
| `%shl`    | 2    | Shift left |
| `%shr`    | 2    | Shift right |

### Missing comparison

| Intrinsic | Args | Description |
|-----------|------|-------------|
| `%ne`     | 2    | Not-equal (numeric-aware) |

### Type predicates (1 arg, all return bool)

| Intrinsic    | Description |
|--------------|-------------|
| `%nil?`      | Is nil? |
| `%empty?`    | Is empty list `()`? |
| `%bool?`     | Is boolean (true or false)? |
| `%int?`      | Is integer? |
| `%float?`    | Is float? |
| `%string?`   | Is string (immutable or mutable)? |
| `%keyword?`  | Is keyword? |
| `%symbol?`   | Is symbol? |
| `%pair?`     | Is pair (cons cell)? |
| `%array?`    | Is array (immutable or mutable)? |
| `%struct?`   | Is struct (immutable or mutable)? |
| `%set?`      | Is set (immutable or mutable)? |
| `%bytes?`    | Is bytes (immutable or mutable)? |
| `%box?`      | Is box (lbox)? |
| `%closure?`  | Is closure? |
| `%fiber?`    | Is fiber? |
| `%type-of`   | Returns type as keyword (`:integer`, `:string`, etc.) |

### Data access

| Intrinsic | Args | Description |
|-----------|------|-------------|
| `%length` | 1    | Polymorphic length (array, string, list, struct, set, bytes) |
| `%get`    | 2    | Indexed/keyed access (array by int, struct by keyword, string by int) |
| `%put`    | 3    | Struct assoc / @array set / @struct put |
| `%del`    | 2    | Struct dissoc / @struct del / set del |
| `%has?`   | 2    | Key/element existence (struct, set, string) |
| `%push`   | 2    | Append element (returns new @array) |
| `%pop`    | 1    | Remove and return last element of @array |

### Mutability (1 arg)

| Intrinsic | Description |
|-----------|-------------|
| `%freeze` | Mutable → immutable copy (array, struct, set, string, bytes) |
| `%thaw`   | Immutable → mutable copy |

### Identity (2 args)

| Intrinsic     | Description |
|---------------|-------------|
| `%identical?` | Bitwise tag+payload equality (pointer identity for heap values) |

## Behavior

Intrinsics have two compilation paths, selected by `--checked-intrinsics`.
The `elle` CLI defaults to **on**; the library/test baseline
(`Config::default`, used by the region/anf unit tests and benches) is off.

### Checked (CLI default): validating native calls

By default the compiler routes `%add` etc. through registered `NativeFn`
primitives. Each primitive validates its argument types and panics on
mismatch with a clear error message including an Elle stack trace:

```
$ elle -e '(%add "a" "b")'
thread 'main' panicked at: +: expected number, got string and string
```

The point is to **audit that your code passes correct types to
intrinsics**. Without it there is no runtime way to catch a bad argument
(short of linting): the unchecked path silently produces garbage, and that
garbage is undebuggable once the code reaches a JIT/SPIR-V/WASM backend.

Checked intrinsics imply `--jit=off --mlir=off`, because JIT/MLIR would
inline the same unchecked native ops and bypass the validation path.
Combining an explicit `--checked-intrinsics` with an explicit
`--jit=eager`/`--mlir` is rejected; use `--checked-intrinsics=off` (which
also restores the optimizing tiers) to opt out.

### Unchecked (`--checked-intrinsics=off`): inline opcodes

With `--checked-intrinsics=off`, intrinsics compile to inline
BinOp/CmpOp/UnaryOp instructions with **no type validation**. Passing
wrong types produces **garbage** (not a crash). This matches WASM
(`I64Add`) and SPIR-V (`OpIAdd`) semantics: the instruction executes on
whatever bits are in the operands.

- `(%add "a" "b")` → `nil` (garbage)
- `(%div 1 0)` → `0` (garbage)
- `(%lt nil 5)` → `false` (garbage)

The signal system sees intrinsics as `Silent` — they never yield, error,
or perform IO. **The caller is responsible for ensuring type safety.** If
the caller cannot guarantee correct types, use the stdlib wrappers.

### Intrinsics as callable values

In **both** modes each `%`-intrinsic is a real `NativeFn`, so a bare `%add`
(anywhere but call position) can be passed to higher-order functions,
stored in data structures, or used in `begin-for-syntax` without stdlib:

```lisp
(def my-add %add)
(assert (= (my-add 10 20) 30) "callable %add")
(assert (= (map (fn [x] (%mul x x)) '(1 2 3)) '(1 4 9)) "intrinsic in map")
```

The modes differ only in call position: checked (the default) compiles
`(%add a b)` to a validating native `Call`; `--checked-intrinsics=off`
compiles it to an inline instruction.

Arithmetic intrinsics operate on integers and floats. Mixed-type operands
follow the same promotion rules as the VM's arithmetic instructions
(integer + float promotes to float).

## Relationship to stdlib

The stdlib wrappers (`+`, `-`, `*`, `/`, `rem`, `mod`, `<`, `>`, `<=`,
`>=`, `not`, `pair`) are Elle functions defined in `stdlib.lisp`. They:

1. Validate argument types at runtime
2. Handle variadic arguments (e.g. `(+ 1 2 3)`)
3. Emit `:error` on type mismatches (catchable, propagates through fibers)
4. Check division by zero before calling `%div`/`%rem`/`%mod`
5. Allocate rest-arg lists for variadic dispatch

Intrinsics bypass all of this. A function using only intrinsics for
arithmetic has signal `Silent` and allocates nothing beyond its own
parameters. **The caller is responsible for ensuring type safety.** If
the caller cannot guarantee correct types, use the stdlib wrappers.
