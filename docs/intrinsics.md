# Intrinsics

<!-- audited: 2026-10-05 -->

Intrinsics are silent bytecode operations prefixed with `%`. A `%`-intrinsic
in **call position** is a compile-time type-checked request for the fast
instruction: the compiler either **proves** the call satisfies the op's
soundness contract and lowers it, or **rejects** the program with a compile
error carrying the call's span. There is no runtime validation in call
position and no unchecked dialect. Three gaps let misuse through today: code
compiled by `eval` skips the proof (#1240), a `(numeric!)` parameter is never
checked against its callers (#1238), and a site after a statement certain to
raise is not checked. Only a resume of that raise reaches the site
([grafting a call](impl/graft.md)).

## When to use intrinsics

Use intrinsics when:
- Writing **hot loops** that must never keep a runtime check. A wrapper call
  over proven operands dissolves to its op (see Lowering), but a wrapper
  whose proof is missing keeps its guards, with no warning. An intrinsic
  whose proof is missing is a compile error.
- Writing code that must be **GPU-eligible** (`%`-intrinsics lower to
  SPIR-V/MLIR; stdlib wrappers do not).
- Writing a **`(silent!)`** function, whose ceiling rejects the `:error` the
  stdlib wrappers may raise.
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

So both of these are rejected at compile time. The helper compiles source
through the file front end and returns the error it reports:

```lisp
(defn rejects [src]
  "The compile error the file front end reports for src."
  (let [[ok? err] (protect (compile/whole-module src "<doc>"))]
    (assert (not ok?) "the source compiles")
    (get err :message)))

# Provably wrong: a string is never a Number.
(assert (string/contains? (rejects "(%add \"a\" 3)")
                          "%add: operand 1 is not a proven number (inferred: string)"))
# Unprovable: x and y have no inferred type here.
(assert (string/contains? (rejects "(defn f [x y] (%add x y))")
                          "%add: operand 1 is not a proven number (inferred: unknown)"))
```

Code that `eval` compiles skips this check today, so misuse there runs
(#1240).

The contract is the op's **full soundness condition** — everything the raw
instruction trusts its operands to satisfy — not only a type:

| Ops | Proof obligation |
|-----|------------------|
| `%add` `%sub` `%mul` · `%lt` `%gt` `%le` `%ge` · `%int` `%float` | operands ⊑ Number |
| `%div` `%rem` `%mod` | operands ⊑ Number **and** the divisor provably nonzero, unless it is a proven Float |
| `%bit-and` `%bit-or` `%bit-xor` `%bit-not` `%shl` `%shr` | operands ⊑ Int |
| `%first` `%rest` | operand is a pair |
| `%length` `%get` `%has?` `%put` `%put-struct[-mut]` `%put-array[-mut]` `%del` `%del-struct[-mut]` `%add-set[-mut]` `%del-set[-mut]` `%pop` `%pop-string` `%pop-bytes` `%array-push` `%push-array[-mut]` `%string-push` `%string-push-mut` `%bytes-push` `%freeze` `%thaw` | container of the op's family (with an index/key the family accepts) |
| `%eq` `%ne` `%identical?` `%not` `%pair` `%type-of` and the type predicates | none — total on every value |

The authoritative per-op table is `check_intrinsic_operand_proofs` in
[the contract check](../src/hir/typeinfer/contract.rs), each row pinned by a
test; this table is the consumer view. Note `%not` is truthiness negation,
total on any value — it needs no proof, exactly like `%eq`.

### What counts as proof

Inference discharges contracts from:

- **literals** and expressions with known types (`(%add 1 2)` compiles);
- **primitive return types** flowing forward (`(%length (->array c))`);
- **a called function's return type** — a call to a function this unit
  defines carries what that function's body was proven to return, a
  recursive call included (see [type inference](impl/typeinfer.md));
- **`match (type-of x)` keyword arms** — inside a `:@array` arm, `x` *is* a
  mutable array, authoritatively;
- **diverging type guards** — after
  `(when (%not (number? b)) (error …))`, `b` is a Number in everything
  that follows; predicate spellings (`(number? b)`) and intrinsic
  spellings (`(%int? b)`) both count, and the raise diverges wherever the
  taken branch evaluates it, inside a `let` binding too. A predicate
  spelling counts only when it names the language's predicate: a parameter
  named `number?` holds whatever the caller passed, and proves nothing;
- **nonzero facts** for the div family — a nonzero literal divisor, or a
  diverging zero guard: after `(when (= d 0) (error …))`, `d` is provably
  nonzero;
- **the result of a proven `%`-intrinsic** — each op's result type is a
  function of its operand types (see below), so one guard proves a whole
  chain of them;
- **a grafted call's body** — a call to a known function becomes that
  function's body (see Lowering), and the body is typed where it stands, so
  `(- n 1)` over an int `n` is the int its `%sub` returns;
- **a `(numeric!)` declaration** — it floors *every parameter of the
  enclosing function* at Number, so a whole numeric kernel proves at once
  without a per-parameter guard. The same declaration asserts that the
  function is GPU-eligible, so a function that makes a call cannot carry it
  (#1239). Nothing checks that a caller passes a number: a string argument
  reaches the `%` sites unchecked (#1238).

```lisp
(def half
  (fn [x]
    "Integer halving on the fast path; rejects non-ints loudly."
    (when (%not (int? x)) (error {:error :type-error :message "half: int required"}))
    (%div x 2)))
(assert (= (half 10) 5) "guard-narrowed %div lowers to the opcode")

# No caller proves x here, so the guard alone must: the raise sits in a
# binding, and the guard still diverges.
(compile/whole-module "(defn parity [x]
                          (when (%not (%int? x))
                            (let [e (error {:error :type-error :message \"no\"})]
                              e))
                          (%rem x 2))"
                      "<doc>")

(defn sq [x]
  "A numeric kernel: the declaration proves the parameter for the whole body."
  (numeric!)
  (%mul x x))
(assert (= (sq 7) 49) "numeric!-declared %mul lowers to the opcode")
(assert (= (map sq [1 2 3]) [1 4 9]) "and dissolves into a fused loop")
```

A parameter takes the join of what its callers in the unit pass. When every
caller passes a proven number, the parameter is proven; one caller passing
anything else makes it unknown again. A lambda parameter with no guard, no
`(numeric!)`, and no proven call sites stays unknown, so
`(fn [x] (%mul x x))` does not compile. Write `(fn [x] (* x x))` (the
wrapper), guard the parameter, or declare `(numeric!)`. The programmer states
the fact once, visibly, and the compiler holds it.

**A definition is checked where it is written.** A function nobody calls has
no proven call sites, so `(defn f [x] (%mul x x))` alone is a compile error,
whatever other forms follow it. "Nothing reaches this parameter" is a claim
about reachability, and no contract row asks that question (see
[type inference](impl/typeinfer.md)).

```lisp
(assert (string/contains? (rejects "(defn f [x] (%mul x x))") "not a proven number"))
(assert (string/contains? (rejects "(defn f [x] (%mul x x))\n(println :other)")
                          "not a proven number"))
(let [module (compile/whole-module "(defn f [x] (%mul x x))\n(f 2)" "<doc>")
      run (get (get module 0) 1)]
  (assert (= 4 (run)) "a caller passing 2 proves x"))
(assert (string/contains? (rejects "(defn f [x] (%mul x x))\n(f 2)\n(f \"s\")")
                          "not a proven number"))
```

The declaration is recorded on the parameter **bindings** it constrains, not
on the function node, so it survives a rewrite that dissolves the function:
`(map sq xs)` over a proven array splices `sq`'s body into an index-walk
loop, and the spliced `%mul` proves against the same floor it proved against
inside `sq` (see [dissolution](impl/dissolution.md)).

### What an op's result proves

A proven op's result carries a type of its own, so the next op reads that
type as its proof and the chain needs no second guard:

```lisp
(defn mask [x]
  "One guard proves the whole chain: the %rem result proves the %bit-and."
  (when (%not (%int? x)) (error {:error :type-error :message "mask: int required"}))
  (%bit-and (%rem x 16) 15))
(assert (= (mask 33) 1) "the remainder of two proven ints is a proven int")
```

| Ops | Result type |
|-----|-------------|
| `%add` `%sub` `%mul` `%div` `%rem` `%mod` | the join of the operand types, clamped to Number: two ints give Int, two floats give Float, a mixed pair gives Number |
| `%bit-and` `%bit-or` `%bit-xor` `%bit-not` `%shl` `%shr` · `%length` · `%int` | Int |
| `%float` | Float |
| `%eq` `%ne` `%lt` `%gt` `%le` `%ge` · `%not` `%identical?` and the type predicates | Bool |
| `%type-of` | Keyword |
| `%pair` | a pair |
| `%push-array[-mut]` `%put-struct[-mut]` `%put-array[-mut]` | the variant's own container type, whatever the operand's mutability |
| `%freeze` `%thaw` | the operand's immutable / mutable twin |
| `%put` `%del` `%del-struct[-mut]` `%add-set[-mut]` `%del-set[-mut]` `%array-push` `%string-push` `%string-push-mut` `%bytes-push` | their first operand's type — the container they store into |
| `%first` `%rest` `%get` `%has?` `%pop` `%pop-string` `%pop-bytes` | nothing: element types are untracked |

The join is what makes the div family usable in an integer kernel. A
remainder over two proven ints is an Int, so it feeds a bitwise op; a
remainder over anything else is not an Int, so the same bitwise op rejects
it. That second case is the one the run time computes as a float, and the
bitwise opcodes read a float's payload as an integer, so admitting it would
return garbage.

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
  (BinOp/CmpOp/UnaryOp/…). These are escape-neutral. The arithmetic,
  comparison, bitwise and conversion ops among them are the ones a GPU kernel
  may hold; they match `I64Add`/`OpIAdd` semantics.
- **Storing and copying ops** — `%put`, `%put-struct[-mut]`,
  `%put-array[-mut]`, `%array-push`, `%push-array[-mut]`, `%string-push[-mut]`,
  `%bytes-push`, `%del`, `%del-struct[-mut]`, `%add-set[-mut]`,
  `%del-set[-mut]`, `%pop`, `%pop-string`, `%pop-bytes`, `%freeze`, `%thaw` — lower to the
  **escape-correct native funnel call** (see
  [the funnel adopt](impl/region/adopt.md)): the same prove-or-reject gate
  for type legality, but
  the store/remove runs through the native whose region accounting records
  cross-region edges and gives the result its call-result region. `%pop`
  rides here so its moved-out element carries that call-result accounting.

A **wrapper** call dissolves where inference proves its operands. The compiler
grafts the body of every known function in place of a call to it, the stdlib
wrappers included, and after inference it deletes the guards and dispatch arms
the operand types decide ([grafting a call](impl/graft.md)). Over proven
operands, what survives is the one op the wrapper's body reaches, and the
operand proofs check it like an op the programmer wrote. Over unproven
operands the guards survive, and the call keeps its validating, signaling,
polymorphic meaning.

```lisp
(defn fhir [src] (get (compile/dumps src "<doc>") :fhir))

(defn occurrences [text part]
  "How many times part occurs in text."
  (- (length (string/split text part)) 1))

# put on a proven @struct keeps the one arm the container's type selects.
(def stored (fhir "(defn g [] (let [s @{}] (put s :x 1) s))\n(g)"))
(assert (string/contains? stored "%put-struct-mut"))
(assert (= 0 (occurrences stored "put#")) "the call to put is gone")
# Every caller passes an int, so both operands are proven and only %add is left.
(def proven (fhir "(defn h [a b] (+ a b))\n(h 1 2)"))
(assert (< 0 (occurrences proven "(%add ")))
(assert (= 0 (occurrences proven "(emit ")) "no guard is left to raise")
# One caller passes a string, so nothing proves a, and the guards stay.
(def guarded (fhir "(defn h [a b] (+ a b))\n(h 1 2)\n(h \"s\" 2)"))
(assert (< 0 (occurrences guarded "(emit ")) "the guards raise at run time")
```

The dissolution changes no result. On two numbers the wrapper's guards cannot
fire, and its body computes what the op computes: the `+` wrapper sums from its
first operand rather than from `0`, so `-0.0` survives both spellings. The
site's signal narrows to silent, because the deleted guards were the only
errors the body could raise.

```lisp
(defn plus [a b] (+ a b))
(assert (= (string (+ -0.0 -0.0)) "-0.0") "the dissolved +")
(assert (= (string (apply plus [-0.0 -0.0])) "-0.0")
        "the wrapper call: plus is used as a value, so nothing proves a or b")
```

A comparison dissolves to its op only over two numbers. `%eq` also compares
two strings, and `%lt` orders them, but the WASM tier compares their payload
words instead of their contents (#1268). So each comparison wrapper tests for
two numbers before it reaches the op, and compares anything else through the
op as a value. A call to `+`, `-` or `*` with three or more operands stays a
call: the general arm of each holds a `letrec` walker, which does not graft. A
comparison's general arm grafts, and compares the operands past the first pair
through `apply`.

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

Wrapping follows from the intrinsic design. `%add` is signal-free, and `+`
dissolves to `%add` wherever both operands are proven numbers (see Lowering). A
type proof cannot exclude overflow, so if overflow were an error the dissolution
would change observable behavior. Wrapping is
the one semantics the VM instruction, the JIT, and the GPU tiers all agree
on. Code that needs overflow detection should check its own ranges (see
`abs` in [the standard library](../src/stdlib.lisp) for the pattern).

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
| `%del-struct` | 2 | Monomorphic del, struct: fresh immutable twin |
| `%del-struct-mut` | 2 | Monomorphic del, @struct: in place, returns arg0 |
| `%add-set` | 2 | Monomorphic add, set: fresh immutable twin |
| `%add-set-mut` | 2 | Monomorphic add, @set: in place, returns arg0 |
| `%del-set` | 2 | Monomorphic del, set: fresh immutable twin |
| `%del-set-mut` | 2 | Monomorphic del, @set: in place, returns arg0 |
| `%has?`   | 2    | Key/element existence (struct, set, string) |
| `%array-push` | 2 | Polymorphic append (returns new array/@array) |
| `%push-array` | 2 | Monomorphic append, array: fresh immutable twin |
| `%push-array-mut` | 2 | Monomorphic append, @array: in place, returns arg0 |
| `%pop`    | 1    | Remove and return last element of @array |
| `%pop-string` | 1 | Remove and return the last grapheme of an @string |
| `%pop-bytes` | 1 | Remove and return the last byte of an @bytes, as an int |
| `%string-push` | 2 | Append a string's bytes (or a char) to string/@string |
| `%string-push-mut` | 2 | Monomorphic append, @string: in place, returns arg0 |
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
`>=`, `=`, `not`, `pair`, `push`, `put`, …) are Elle functions defined in
[the standard library](../src/stdlib.lisp). They:

1. Validate argument types at runtime and accept polymorphic inputs
2. Handle variadic arguments (e.g. `(+ 1 2 3)`)
3. Emit `:error` on type mismatches (catchable, propagates through fibers)
4. Check division by zero before dividing
5. Allocate a rest list for a variadic call that is not grafted

Internally the wrappers use `%`-intrinsics on operands their own guards
have proven — the guard that raises the wrapper's `:error` is the same
fact that discharges the intrinsic's contract on the fall-through path.
That is the intended shape for user code too: validate at the boundary,
compute with intrinsics inside. It is also what lets a grafted wrapper
dissolve: the proof the guard would have made is already in hand, so the
guard goes and the op stays.
