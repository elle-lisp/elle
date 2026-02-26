# Janet's Destructuring

A design reference for language designers. Janet has two completely separate
destructuring systems: a **compiler-level** system used by binding forms, and
a **macro-level** pattern matcher. Understanding both — and why they are
separate — is instructive for language design.


## The Two Systems

| | Compiler `destructure()` | Macro `match` |
|---|---|---|
| Implemented in | C (specials.c) | Janet (boot.janet) |
| Used by | `def`, `var`, `fn`, `let`, `each`, `loop`, `with` | `match` only |
| Literal matching | No (compile error) | Yes |
| Wildcard `_` | No (binds to `_`) | Yes (no binding) |
| Guard clauses | No | Yes |
| Quoted literals `'x` | No | Yes |
| Unification `(@ sym)` | No | Yes |
| Local unification `[x x]` | No (binds twice) | Yes (checks equality) |
| Exact length `$` | No | Yes |
| `& rest` | Yes | Yes |
| Nested patterns | Yes | Yes |
| On mismatch | Silent nil | Falls through to next case |

The compiler system is unconditional extraction — it always succeeds, binding
nil for missing values. The macro system is conditional matching — it tests
whether the value fits the pattern and branches accordingly.


## Compiler-Level Destructuring

### The Core Function

`destructure()` in specials.c is a tree-walk over the pattern structure. At
each internal node (tuple, array, struct, table), it emits bytecode to extract
a sub-value, then recurses. At each leaf (symbol), it calls a **leaf callback**
that performs the actual binding.

```
destructure(compiler, pattern, value_slot, leaf_callback, attr_table) → can_free
```

The leaf callback is the **strategy pattern** that differentiates `def` from
`var`:

| Form | Leaf Callback | Behavior |
|------|---------------|----------|
| `def` | `defleaf` | Immutable binding (no `MUTABLE` flag) |
| `var` | `varleaf` | Mutable binding (`MUTABLE` flag, allows `set`) |

The destructuring logic itself is identical in both cases — only the leaf
behavior differs.

### Pattern Dispatch

The function switches on the Janet type of the pattern:

**Symbol** — the base case. Calls `leaf(compiler, symbol, slot, attrs)`.
Returns whatever the leaf returns (the leaf may claim the slot by aliasing it,
preventing the caller from freeing it).

**Tuple or Array** — indexed destructuring. For each element at index `i`:

1. Allocate a temporary slot
2. Emit `GET_INDEX temp, source, i` (or `IN` if index ≥ 256)
3. Recurse into the sub-pattern with `temp` as the value
4. Free the temporary

**Struct or Table** — dictionary destructuring. For each key-value pair in the
pattern:

1. Allocate a temporary slot
2. Compile the **key** as an expression (keys can be arbitrary expressions)
3. Emit `IN temp, source, key`
4. Recurse into the **value** part of the pair as a sub-pattern
5. Free the temporary

**Anything else** — compile error. Numbers, strings, keywords, booleans, and
nil are not valid patterns in binding destructuring.

### The `&` Rest Pattern

When a `&` is encountered in an indexed pattern, it must be followed by
exactly one symbol. The compiler emits an inline loop:

```
LOAD_INTEGER  counter, i           # start at current index
LENGTH        len, source          # get source length
loop:
LESS_THAN     tmp, counter, len    # counter < len?
JUMP_IF_NOT   tmp, exit
GET           tmp, source, counter # get element
PUSH          tmp                  # push onto stack
ADD_IMMEDIATE counter, counter, 1
JUMP          loop
exit:
MAKE_TUPLE    result               # collect into tuple
```

The rest value is always a **tuple** (immutable), regardless of whether the
source was an array.

**Design note**: The rest collection emits 6 instructions per iteration plus
setup/teardown. For a language design, consider whether this inline loop is
worth the code size vs. a runtime helper function. Janet chose inline emission,
which avoids a function call but produces ~18 instructions for a typical rest
binding.

### Nested Patterns

Patterns nest naturally because `destructure()` recurses. `[[a b] {c :c}]`
first extracts index 0, recurses into `[a b]` (which extracts indices 0 and
1), then extracts index 1, recurses into `{c :c}` (which looks up key `:c`).

Temporaries stack during nesting. `[[a b] [c d]]` at peak has 2 temporaries
live (one per nesting level). Each temporary is freed after its sub-pattern
is fully processed.

### What Destructuring Does NOT Check

There are no runtime checks. If the value does not match the pattern:

- Missing indexed element → `GET_INDEX` returns nil
- Missing dictionary key → `IN` returns nil
- Wrong type entirely → `GET_INDEX` on a non-indexed type returns nil
- `& rest` on a short value → loop runs zero iterations, produces empty tuple

This is the **silent nil** model. Destructuring always succeeds at runtime.
This is appropriate for binding forms where the programmer is asserting the
structure, not testing it.


## The Optimization Layer

Between `def`/`var` and `destructure()` sits `dohead_destructure()`, an
optimization that avoids constructing intermediate values when both sides are
indexed literals.

### When It Applies

Three conditions must all be true:

1. The result of the `def`/`var` expression is unused (`DROP` flag)
2. The LHS is a tuple or array (indexed pattern)
3. The RHS is an array or bracket tuple (indexed literal, not a function call)

Additionally, the LHS must not contain `&`, and the RHS must not contain
`splice`.

### What It Does

Instead of compiling the RHS into a single value and then destructuring it,
the optimizer **pairs up** LHS and RHS elements directly and recurses:

```
(def [a b c] [x y z])
```

Without optimization: compile `[x y z]` into a tuple (3 pushes + 1
`MAKE_BRACKET_TUPLE`), then 3 `GET_INDEX` instructions. Total: ~7 instructions.

With optimization: compile `x`, `y`, `z` individually and bind directly.
Total: ~3 instructions (or 0 if they are constants that can be aliased).

The optimizer recurses, so nested patterns work:

```
(def [[a b] [c d]] [[1 2] [3 4]])
```

Each inner pair is optimized independently — the outer tuple and inner tuples
are never constructed.

**Design note**: This optimization matters for the common pattern of
multiple-return-value destructuring. It is a compile-time transformation that
eliminates temporary aggregate construction. For a language design, this is
worth having — it makes destructuring zero-cost for the common case.


## How `def` and `var` Use Destructuring

### `def` at Top-Level Scope

1. Parse metadata via `handleattr()` (keywords, docstrings, structs)
2. Run `dohead_destructure()` to produce `(pattern, slot)` pairs
3. For each pair, call `destructure()` with `defleaf` as the callback

`defleaf` at top scope:

1. Clone the attribute table into a new environment entry
2. Add `:source-map`
3. If `:redef` is enabled: use a ref array (allows runtime redefinition)
4. Otherwise: emit `PUT entry, :value, computed_value` — bytecode that stores
   the value into the entry table when the thunk runs
5. Add the entry to the compile-time environment immediately (so later forms
   can see the binding before the thunk executes)

### `def` at Inner Scope

`defleaf` calls `namelocal()`, which:

1. If the slot is unnamed, immutable, and local → **alias** it (no instruction)
2. Otherwise → allocate a register, emit `MOV`, name it

The aliasing optimization means `(def x existing-def)` is zero-cost.

### `var` Differences

| Aspect | `def` | `var` |
|--------|-------|-------|
| Leaf callback | `defleaf` | `varleaf` |
| Local slot flags | immutable | `MUTABLE` |
| Top-level storage | `:value` in entry (or `:ref` if redef) | Always `:ref` array |
| Can be target of `set` | No | Yes |
| Alias optimization | Yes | No (mutable prevents it) |

### Metadata on Destructured Bindings

The attribute table from `handleattr()` is passed through `destructure()` to
every leaf. This means metadata applies to **all** bindings:

```janet
(def :private [a b] [1 2])
```

Both `a` and `b` get `:private`. This is correct but potentially surprising —
there is no way to apply metadata to individual bindings within a destructuring
pattern.


## How `fn` Parameters Use Destructuring

Function parameters are processed in two phases:

### Phase 1 — Register Allocation

Parameters are iterated left to right. Each gets a register:

- **Symbol**: Named immediately (`janetc_nameslot`)
- **Non-symbol** (tuple, struct, etc.): Register allocated but NOT named.
  The pattern and register are saved to a `destructed_params` list.
- **`&`**: Marks the vararg position
- **`&opt`**: Sets minimum arity (subsequent params are optional)
- **`&keys`**: Collects remaining args as a struct
- **`&named`**: Named argument destructuring with a lookup table

### Phase 2 — Destructuring

After all registers are allocated (so the VM knows the function's arity and
can fill the registers with arguments), the compiler iterates over
`destructed_params` and calls `destructure()` on each:

```
destructure(compiler, pattern, register_slot, defleaf, NULL)
```

Parameters are always immutable (`defleaf`), never `var`.

### Example

```janet
(fn [[a b] c] (+ a b c))
```

1. Parameter 0 (`[a b]`): allocate register 0, save to `destructed_params`
2. Parameter 1 (`c`): allocate register 1, name it `c`
3. Destructuring: `destructure([a b], reg0, defleaf)`:
   - `GET_INDEX reg2, reg0, 0` → name `a`
   - `GET_INDEX reg3, reg0, 1` → name `b`
4. Body: `(+ a b c)` uses reg2, reg3, reg1

The destructuring code runs at the start of the function body, after the VM
has filled registers 0 and 1 with the actual arguments. This means
destructuring in parameters has the same runtime cost as destructuring in
`def` — there is no special-casing.


## How `match` Works (The Macro System)

`match` is defined entirely in Janet (boot.janet, ~200 lines). It is a macro
that compiles pattern-value pairs into a chain of `if` expressions. It does
NOT use the compiler's `destructure()` function.

### Compilation Strategy

Each pattern is processed in two passes:

**Pass 1 — Binding collection** (`visit-pattern-1`): Walks the pattern,
emitting `def` forms to extract sub-values into gensyms. Uses memoization —
if the same path (parent + key) is extracted by multiple patterns, the gensym
is reused.

**Pass 2 — Condition collection** (`visit-pattern-2`): Walks the pattern
again, collecting boolean conditions that must all be true for the pattern to
match.

The accumulated defs and conditions are then assembled into:

```janet
(do
  (def gs1 (get source 0))
  (def gs2 (get source 1))
  ...
  (if (and cond1 cond2 ...)
    (do (def user-sym gs1) ... body)
    <next-pattern>))
```

### Pattern Types in `match`

**Symbol** — always matches. Binds the value.

**`_` wildcard** — always matches. Binds nothing.

**Quoted literal `'x`** — emits `(= value x)`.

**Unquoted literal** (number, string, keyword, boolean, nil) — emits
`(= value literal)`. This is what `destructure()` cannot do.

**Array or bracket tuple `[a b c]`** — emits:
- Length extraction: `(def len-gs (if (indexed? source) (length source)))`
- Length check: `(<= pattern-length len-gs)`
- Sub-value extraction: `(def gs (get source i))` for each element
- Recurse into each sub-pattern

**`$` exact-length marker** — changes the length check from `<=` to `=`.
Must be the last element. `[a b $]` matches only 2-element indexed values.

**`&` rest** — emits `(def rest-gs (slice source i))` for the rest.
Unlike the compiler's inline loop, this uses the `slice` function.

**Struct or table `{:x pat1 :y pat2}`** — for each key:
- Extracts: `(def gs (get source key))`
- Existence check: `(not= nil gs)`
- Recurse into the value pattern

**Guard clause `(pattern pred1 pred2 ...)`** — the first element is the
actual pattern (recursed into). Remaining elements are predicate expressions,
added directly to the condition list. The predicates can reference bindings
from the pattern:

```janet
(match x
  (n (even? n)) (print "even")
  n (print "odd"))
```

**`(@ sym)` global unification** — binds `sym` across the entire match form.
If `(@ sym)` appears in multiple patterns (or multiple times in one pattern),
all occurrences must be equal. This is a form of join:

```janet
(match [a b]
  [(@ x) (@ x)] (print "equal")
  _ (print "different"))
```

**Local unification** — if the same plain symbol appears twice in a pattern,
all occurrences must be equal:

```janet
(match [1 1]
  [x x] (print "same")   # matches
  _ (print "different"))
```

This works because `visit-pattern-1` accumulates gensyms per symbol name, and
when a symbol has multiple gensyms, an `(= gs1 gs2 ...)` condition is added.

### Memoization

The `get-sym` helper caches sub-value extractions in a table keyed by
`[parent-sym key]`. If the same path is needed by multiple patterns or
multiple parts of the same pattern, the gensym is reused. This avoids
redundant `get` calls.

Similarly, `get-length-sym` caches length extractions per parent symbol.

### Branch Assembly

After all patterns are processed, the accumulated `(condition, body)` pairs
are assembled into nested `if` expressions, processed in reverse to build
the chain from the inside out.

If no pattern matches, the result is nil (no error).


## How Other Forms Inherit Destructuring

### `let`

```janet
(defmacro let [bindings & body]
  ...)
```

Expands each pair into `(def k v)` wrapped in `(do ...)`. Since `def` supports
destructuring, `let` inherits it for free:

```janet
(let [[a b] [1 2]
      {:x x} {:x 3}]
  (+ a b x))
```

### `each`

```janet
(each [k v] (pairs my-table) ...)
```

The binding is placed in a `(def ,binding (in ds k))` form inside the
expansion. Since `def` supports destructuring, `each` supports it.

### `loop`

All loop binding forms (`:in`, `:range`, `:iterate`, `:keys`, `:pairs`)
use `(def ,binding ...)` internally, so all support destructuring.

The `:let` modifier also expands to `let`, which supports destructuring.

### `with`

```janet
(with [[a b] (get-pair) close-pair] ...)
```

The binding in `with` is used in a `def` form, so it supports destructuring.

### `defer`

`defer` wraps the body in a fiber, not a binding form. No destructuring.

### `for`

The loop variable in `for` is bound via `def` inside `for-var-template`.
Technically it supports destructuring, but since the value is always a number,
this is not useful.

### General Principle

Any macro that expands to `def` or `var` automatically inherits destructuring.
This is the composition payoff — destructuring is not a property of individual
forms, it is a property of the `def` and `var` special forms. All forms built
on top of them get it for free.


## `handleattr()` — Metadata in Binding Forms

The `handleattr()` function parses metadata between the binding name and the
value in `def`/`var`:

```janet
(def :private :doc "my doc" [a b] [1 2])
```

It processes arguments between the pattern and the value:
- **Keyword**: Added as `keyword → true` (e.g., `:private`)
- **String**: Added as `:doc` metadata
- **Struct**: Merged into the attribute table
- **Tuple**: Compile error ("did you intend to use `defn`?")

The resulting attribute table is passed through to every leaf binding. When
the pattern is a symbol, the binding name is used for error messages. When the
pattern is a destructuring form, the generic `"<multiple bindings>"` is used.


## Performance Analysis

### Instruction Counts

| Pattern | Instructions | Notes |
|---------|-------------|-------|
| `(def a val)` | 0–1 | 0 if aliased, 1 `MOV` otherwise |
| `(def [a b] val)` | 2 | 2 `GET_INDEX` |
| `(def [a b c] [1 2 3])` | 0 | Optimized away by `dohead_destructure` |
| `(def {:x x :y y} val)` | 2 | 2 `IN` (plus 2 constant loads for keys) |
| `(def [a & rest] val)` | ~18 | 1 `GET_INDEX` + loop (~6/iter) + `MAKE_TUPLE` |
| `(def [[a b] [c d]] val)` | 4 | 2 outer `GET_INDEX` + 2 inner `GET_INDEX` |

### Temporary Register Usage

At any nesting level, `destructure()` has at most 1 temporary live (the
`nextright` slot for the current element). The `& rest` pattern temporarily
uses 4 registers (counter, temp, length, result). Temporaries are freed
eagerly after each element.

### `match` vs `destructure()`

`match` generates more code because it must check conditions. Each pattern
case produces: N `def` forms for extraction + M condition checks + branching.
The memoization of `get-sym` prevents redundant extraction across patterns,
but there is no sharing of condition checks.

For a language design, consider whether `match` should desugar to the same
destructuring primitive with added conditions, or remain a separate system.
Janet chose separation — the compiler system is fast and simple, the macro
system is powerful and independent.


## Design Principles to Extract

### 1. The Leaf Callback Pattern

Destructuring is parameterized by a callback that handles the terminal case
(binding a symbol to a value). This cleanly separates the structural
decomposition logic from the binding semantics. `def` and `var` share 100%
of the destructuring logic and differ only in the leaf.

For a language design: if you have multiple binding forms with different
semantics (immutable, mutable, typed, validated), a callback-parameterized
destructuring core handles all of them.

### 2. Silent Nil vs. Conditional Matching

Janet's binding destructuring always succeeds, binding nil for missing values.
Pattern matching (`match`) conditionally tests and falls through.

These are fundamentally different operations. Binding destructuring is an
assertion: "I know the shape, give me the pieces." Pattern matching is a
query: "Does this value fit this shape?"

Trying to unify them (as some languages do) creates tension. If destructuring
can fail, every `def` needs error handling. If it cannot fail, `match` needs
a separate mechanism. Janet's choice to keep them separate is clean.

### 3. Composition Through `def`

Because destructuring lives in the `def` and `var` special forms, every macro
that expands to `def` or `var` inherits it automatically. `let`, `each`,
`loop`, `with`, and user-defined macros all get destructuring for free.

For a language design: put destructuring in the lowest-level binding primitive.
Everything above it benefits. Do not put destructuring in `let` or `match`
and then try to propagate it downward — that is backwards.

### 4. The Indexed Literal Optimization

`dohead_destructure()` eliminates intermediate aggregate construction when
both sides are known at compile time. This is significant because the most
common destructuring pattern is `(def [a b] [expr1 expr2])` — returning
multiple values.

For a language design: if your destructuring compiles to extraction from an
aggregate, make sure you optimize away the aggregate when it is a literal.
Otherwise, every multiple-return-value pattern pays for an allocation that is
immediately discarded.

### 5. `&` Rest as Inline Loop vs. Runtime Helper

The compiler emits an inline loop for `& rest` (~6 instructions per
iteration). The `match` macro uses a `slice` call instead.

The trade-off: inline loops avoid function call overhead but produce more
bytecode. A runtime `slice` helper is smaller but pays the call cost.

For a language design: if rest patterns are rare and performance-critical,
inline. If they are common and code size matters, use a helper. Consider
whether rest values should be the same type as the source (Janet always
produces a tuple) or a fresh allocation of the source type.

### 6. Dictionary Destructuring Keys Are Expressions

In Janet, dictionary pattern keys are compiled as arbitrary expressions:

```janet
(def {(keyword k) v} my-struct)
```

This is more powerful than restricting keys to literals, but means the
compiler must emit code for key computation even when the key is a simple
keyword. For the common case `{:x x}`, the keyword `:x` is compiled as a
constant load — not a significant cost.

For a language design: consider whether dictionary destructuring keys should
be restricted to literals (simpler, enables more optimizations) or allowed
as expressions (more powerful, harder to optimize).

### 7. No Default Values in Compiler Destructuring

Janet's compiler destructuring has no syntax for default values. Missing
values are nil. The `match` macro also has no default-value syntax per
binding — you can only fall through to a different pattern.

Default values (e.g., `(def [a (b 0)] [1])` where `b` defaults to 0) are a
common feature in other languages. Janet's omission keeps the destructuring
logic simple. Users who need defaults can use `(default b 0)` after the
destructuring.

For a language design: default values in destructuring patterns add
complexity (conditional logic, evaluation order questions, interaction with
rest patterns). Consider whether the simplicity of "missing = nil" plus a
post-hoc default form is sufficient.

### 8. `_` Is Not a Wildcard

In Janet's compiler destructuring, `_` is a regular symbol. `(def [_ b] xs)`
binds `_` to the first element. The compiler may emit an unused-variable
lint warning.

In `match`, `_` is a true wildcard — it matches anything and binds nothing.

This inconsistency is minor in practice (binding an unused `_` is harmless)
but worth noting for a language design. If `_` is special, make it special
everywhere. If it is not, make it clear that it is a normal binding.

### 9. No Type Checking in Destructuring

The compiler does not check that the source value is the right type for the
pattern. `(def [a b] 42)` compiles without error and at runtime `a` and `b`
are both nil (`GET_INDEX` on a number returns nil).

For a language design with static types: destructuring is a natural place to
insert type checks (at compile time) or type assertions (at runtime). Janet's
dynamic typing makes this a non-issue, but a typed language should consider
how destructuring patterns interact with the type system.

### 10. Two Systems Is Fine

Having both compiler-level destructuring and macro-level pattern matching is
not redundant — they serve different purposes. The compiler system is fast,
simple, and unconditional. The macro system is powerful, conditional, and
expressive. Trying to merge them would complicate both.

For a language design: do not be afraid of having two destructuring systems
if they serve genuinely different purposes. The key is that the simpler one
should be the primitive (used by `def`, `let`, etc.) and the more complex one
should be built on top (used by `match`). Ideally, the complex one should be
expressible as a macro over the simple one — which is exactly what Janet does.
