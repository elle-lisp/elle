# Intrinsics

Intrinsics are silent bytecode operations prefixed with `%`. A `%`-intrinsic
in **call position** is a compile-time type-checked request for the fast
instruction: the compiler either **proves** the call satisfies the op's
soundness contract and lowers it, or **rejects** the program with a compile
error carrying the call's span. Misuse is unrepresentable in compiled code —
there is no runtime validation in call position and no unchecked dialect.

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

Use stdlib wrappers (`+`, `-`, `*`, etc.) in all other code. They are
polymorphic, validate inputs at runtime, produce clear error messages, and
handle mixed int/float promotion. An intrinsic call the compiler cannot
prove is a compile error, not a slower fallback — when the operand types
are honestly dynamic, the wrapper is the right spelling.

## The contract: prove or reject

`(%foo a b)` compiles iff the inferred operand types discharge `%foo`'s
contract at that site:

- **proven** → the call lowers (see Lowering) and the site is silent;
- **provably wrong** or **unprovable** → compile error.

So both of these are rejected at compile time:

```text
(%add "a" 3)      ; provably wrong: a string is never a Number
(%add x y)        ; unprovable: x and y have no inferred type here
```

The contract is the op's **full soundness condition** — everything the raw
instruction trusts its operands to satisfy — not only a type:

| Ops | Proof obligation |
|-----|------------------|
| `%add` `%sub` `%mul` · `%lt` `%gt` `%le` `%ge` · `%int` `%float` | operands ⊑ Number |
| `%div` `%rem` `%mod` | operands ⊑ Number **and** the divisor provably nonzero |
| `%bit-and` `%bit-or` `%bit-xor` `%bit-not` `%shl` `%shr` | operands ⊑ Int |
| `%first` `%rest` | operand is a pair |
| `%length` `%get` `%has?` `%put` `%put-struct[-mut]` `%put-array[-mut]` `%del` `%pop` `%array-push` `%push-array[-mut]` `%string-push` `%bytes-push` `%freeze` `%thaw` | container of the op's family (with an index/key the family accepts) |
| `%eq` `%ne` `%identical?` `%not` `%pair` `%type-of` and the type predicates | none — total on every value |

The authoritative per-op table is `check_intrinsic_operand_proofs`
(`src/hir/typeinfer/`), each row pinned by a test; this table is the
consumer view. Note `%not` is truthiness negation, total on any value —
it needs no proof, exactly like `%eq`.

### What counts as proof

Inference discharges contracts from:

- **literals** and expressions with known types (`(%add 1 2)` compiles);
- **primitive return types** flowing forward (`(%length (->array c))`);
- **`match (type-of x)` keyword arms** — inside a `:@array` arm, `x` *is* a
  mutable array, authoritatively;
- **diverging type guards** — after
  `(when (%not (number? b)) (error …))`, `b` is a Number in everything
  that follows; predicate spellings (`(number? b)`) and intrinsic
  spellings (`(%int? b)`) both count;
- **nonzero facts** for the div family — a nonzero literal divisor, or a
  diverging zero guard: after `(when (= d 0) (error …))`, `d` is provably
  nonzero;
- **a `(numeric!)` declaration** — it floors *every parameter of the
  enclosing function* at Number, so a whole numeric kernel proves at once
  without a per-parameter guard.

```lisp
(def half
  (fn [x]
    "Integer halving on the fast path; rejects non-ints loudly."
    (when (%not (int? x)) (error {:error :type-error :message "half: int required"}))
    (%div x 2)))
(assert (= (half 10) 5) "guard-narrowed %div lowers to the opcode")

(defn sq [x]
  "A numeric kernel: the declaration proves the parameter for the whole body."
  (numeric!)
  (%mul x x))
(assert (= (sq 7) 49) "numeric!-declared %mul lowers to the opcode")
(assert (= (map sq [1 2 3]) [1 4 9]) "and dissolves into a fused loop")
```

A lambda parameter with no guard, no `(numeric!)`, and no proven call sites
stays unknown, so `(fn [x] (%mul x x))` does not compile — write
`(fn [x] (* x x))` (the wrapper), guard the parameter, or declare
`(numeric!)`. This is the point of the design: the programmer states the
fact once, visibly, and the compiler holds it.

The declaration is recorded on the parameter **bindings** it constrains, not
on the function node, so it survives a rewrite that dissolves the function:
`(map sq xs)` over a proven array splices `sq`'s body into an index-walk
loop, and the spliced `%mul` proves against the same floor it proved against
inside `sq` (`docs/impl/dissolution.md` § "Raw `%`-intrinsic bodies").

### Silent by construction

The signal system sees a lowered intrinsic as `Silent` — it never yields,
errors, or performs IO. The proof is what buys the silence: an operand the
op would misbehave on cannot reach it. Division is the sharpest case — a
compiled `%div` site cannot divide by zero because the nonzero obligation
is discharged at compile time; the `/`·`rem`·`mod` **wrappers** are where a
dynamic divisor is checked and `:error` is raised.

## Lowering

One lowering per op; which one is a fixed property of the op, not a mode:

- **Non-storing ops** — arithmetic, comparison, logic, bitwise, conversion,
  predicates, `%pair`, `%first`, `%rest`, `%get`, `%length`, `%has?`,
  `%type-of`, `%identical?` — lower to **one VM instruction**
  (BinOp/CmpOp/UnaryOp/…). These are escape-neutral and GPU/WASM-eligible;
  this is the path that matches `I64Add`/`OpIAdd` semantics.
- **Storing and copying ops** — `%put`, `%put-struct[-mut]`,
  `%put-array[-mut]`, `%array-push`, `%push-array[-mut]`, `%string-push`,
  `%bytes-push`, `%del`, `%pop`, `%freeze`, `%thaw` — lower to the
  **escape-correct native funnel call** (`docs/impl/region/adopt.md`,
  § The funnel adopt): the same prove-or-reject gate for type legality, but
  the store/remove runs through the native whose region accounting records
  cross-region edges and gives the result its call-result region. `%pop`
  rides here so its moved-out element carries that call-result accounting.

A **wrapper** call is never rewritten to the instruction: `(+ a b)` is the
programmer's explicit request for the validating, signaling, polymorphic
surface, and it keeps that meaning at every site. The fast path is spelled
`%add` — and proven.

## Intrinsics as callable values

Every `%`-intrinsic is also a registered `NativeFn`, so a bare `%add`
(anywhere but call position) can be passed to higher-order functions,
stored in data structures, or used in `begin-for-syntax`. A dynamic call
through the value validates its arguments at runtime — no operand types
exist at the call site, so there is nothing to prove and the native checks
instead:

```lisp
(def my-add %add)
(assert (= (my-add 10 20) 30) "callable %add")
(assert (= (reduce %add 0 '(1 2 3)) 6) "intrinsic as a value in a HOF")
```

Arithmetic intrinsics operate on integers and floats. Mixed-type operands
follow the same promotion rules as the VM's arithmetic instructions
(integer + float promotes to float).

## Integer overflow

Integers are 64-bit two's-complement and **wrap on overflow**, on every
tier:

```lisp
(assert (= (+ 9223372036854775807 1) -9223372036854775808) "wraps")
```

This is a consequence of the intrinsic design, not an accident. `%add` is
signal-free, and the compiler emits it for `+` whenever both operands are
proven ints — but a type proof cannot exclude overflow, so if overflow were
an error the specialization would change observable behavior. Wrapping is
the one semantics the VM instruction, the JIT, and the GPU tiers all agree
on. Code that needs overflow detection should check its own ranges (see
`abs` in stdlib.lisp for the pattern).

Integer division and remainder by zero are errors in the stdlib wrappers;
`(/ i64-min -1)` and `(rem i64-min -1)` wrap (to `i64-min` and `0`).

## Complete list

### Arithmetic

| Intrinsic | Args | Description |
|-----------|------|-------------|
| `%add`    | 2    | Addition |
| `%sub`    | 1-2  | Subtraction; `(%sub x)` negates |
| `%mul`    | 2    | Multiplication |
| `%div`    | 2    | Division (divisor must be proven nonzero) |
| `%rem`    | 2    | Remainder, sign follows dividend (divisor proven nonzero) |
| `%mod`    | 2    | Modulo, sign follows divisor (divisor proven nonzero) |

### Comparison

| Intrinsic | Args | Description |
|-----------|------|-------------|
| `%eq`     | 2    | Equality |
| `%ne`     | 2    | Not-equal (numeric-aware) |
| `%lt`     | 2    | Less than |
| `%gt`     | 2    | Greater than |
| `%le`     | 2    | Less than or equal |
| `%ge`     | 2    | Greater than or equal |

### Logic and conversion

| Intrinsic | Args | Description |
|-----------|------|-------------|
| `%not`    | 1    | Truthiness negation (total on any value) |
| `%int`    | 1    | Convert to integer (truncates floats) |
| `%float`  | 1    | Convert to float |

### Pairs

| Intrinsic | Args | Description |
|-----------|------|-------------|
| `%pair`   | 2    | Construct a pair cell |
| `%first`  | 1    | First element of a pair |
| `%rest`   | 1    | Rest of a pair |

### Bitwise

| Intrinsic | Args | Description |
|-----------|------|-------------|
| `%bit-and`| 2    | Bitwise AND |
| `%bit-or` | 2    | Bitwise OR |
| `%bit-xor`| 2    | Bitwise XOR |
| `%bit-not`| 1    | Bitwise complement |
| `%shl`    | 2    | Shift left |
| `%shr`    | 2    | Shift right |

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

### Data access and mutation

| Intrinsic | Args | Description |
|-----------|------|-------------|
| `%length` | 1    | Polymorphic length (array, string, list, struct, set, bytes) |
| `%get`    | 2    | Indexed/keyed access (array by int, struct by keyword, string by int) |
| `%put`    | 3    | Polymorphic put (struct assoc / @array set / @struct put) |
| `%put-struct` | 3 | Monomorphic put, struct: fresh immutable twin |
| `%put-struct-mut` | 3 | Monomorphic put, @struct: in place, returns arg0 |
| `%put-array` | 3 | Monomorphic put, array: fresh immutable twin |
| `%put-array-mut` | 3 | Monomorphic put, @array: in place, returns arg0 |
| `%del`    | 2    | Struct dissoc / @struct del / set del |
| `%has?`   | 2    | Key/element existence (struct, set, string) |
| `%array-push` | 2 | Polymorphic append (returns new array/@array) |
| `%push-array` | 2 | Monomorphic append, array: fresh immutable twin |
| `%push-array-mut` | 2 | Monomorphic append, @array: in place, returns arg0 |
| `%pop`    | 1    | Remove and return last element of @array |
| `%string-push` | 2 | Append a string's bytes (or a char) to string/@string |
| `%bytes-push` | 2 | Append an integer byte, or all bytes of a bytes value, to bytes/@bytes |

### Mutability

| Intrinsic | Args | Description |
|-----------|------|-------------|
| `%freeze` | 1    | Mutable → immutable copy (array, struct, set, string, bytes) |
| `%thaw`   | 1    | Immutable → mutable copy |

### Identity

| Intrinsic     | Args | Description |
|---------------|------|-------------|
| `%identical?` | 2    | Bitwise tag+payload equality (pointer identity for heap values) |

## Relationship to stdlib

The stdlib wrappers (`+`, `-`, `*`, `/`, `rem`, `mod`, `<`, `>`, `<=`,
`>=`, `not`, `pair`, `push`, `put`, `get`, …) are Elle functions defined in
`stdlib.lisp`/`core.lisp`. They:

1. Validate argument types at runtime and accept polymorphic inputs
2. Handle variadic arguments (e.g. `(+ 1 2 3)`)
3. Emit `:error` on type mismatches (catchable, propagates through fibers)
4. Check division by zero before dividing
5. Allocate rest-arg lists for variadic dispatch

Internally the wrappers use `%`-intrinsics on operands their own guards
have proven — the guard that raises the wrapper's `:error` is the same
fact that discharges the intrinsic's contract on the fall-through path.
That is the intended shape for user code too: validate at the boundary,
compute with intrinsics inside.
