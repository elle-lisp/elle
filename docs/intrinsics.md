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

### Bitwise (2 args)

| Intrinsic | Args | Description |
|-----------|------|-------------|
| `%bit-and`| 2    | Bitwise AND |
| `%bit-or` | 2    | Bitwise OR |
| `%bit-xor`| 2    | Bitwise XOR |
| `%shl`    | 2    | Shift left |
| `%shr`    | 2    | Shift right |

## Behavior

Intrinsics have **no runtime validation**. Passing the wrong types
(e.g. `(%add "a" "b")`) produces undefined behavior, not a clean error.
They never emit signals, so their compile-time signal is always `Silent`.

Arithmetic intrinsics operate on integers and floats. Mixed-type operands
follow the same promotion rules as the VM's arithmetic instructions
(integer + float promotes to float).

## Relationship to stdlib

The stdlib wrappers (`+`, `-`, `*`, `/`, `rem`, `mod`, `<`, `>`, `<=`,
`>=`, `not`, `pair`) are Elle functions defined in `stdlib.lisp`. They:

1. Validate argument types at runtime
2. Handle variadic arguments (e.g. `(+ 1 2 3)`)
3. Emit `:error` on type mismatches
4. Allocate rest-arg lists for variadic dispatch

Intrinsics bypass all of this. A function using only intrinsics for
arithmetic has signal `Silent` and allocates nothing beyond its own
parameters.
