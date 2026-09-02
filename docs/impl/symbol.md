# Symbols and keywords — identity is the name hash

A `SymbolId` is the 64-bit FNV-1a hash of the symbol's name. Nothing mints it
and no table owns it: the same name yields the same id in every symbol table,
every thread, every process, and every build.

```rust
pub struct SymbolId(pub u64);
SymbolId::of("map") == SymbolId::of("map")   // always, everywhere
```

A keyword's payload is the same hash of the same name, through the same
function (`src/namehash.rs`), so `TAG_SYMBOL` and `TAG_KEYWORD` carry the same
kind of payload: a name hash, not an index. The tag alone separates the two
vocabularies; `map` and `:map` are different values with equal payloads.

## What the property buys

A symbol value is portable. It survives a copy to another thread, a write to a
file, and a hydration into a fresh process, because its bytes mean the same
thing on both sides. Everything that used to compensate for a process-local id
is gone: no per-code-object `symbol_names` map, no re-interning of symbol data
crossing a thread boundary, and no rule that two symbol tables must intern the
same names in the same order to agree.

Sorted containers get the property for free. An immutable struct and an
immutable set are sorted arrays, and `TableKey`/`Value` order symbols by raw
id. Under mint order that arrangement was correct only in the process that
built it. Under hash order it is correct everywhere. The order is hash order,
not alphabetical: sorting by name would cost a memo lookup on every
comparison, and every struct probe is a binary search over those comparisons.
Build and probe share the comparator, so the containers stay correct; the
order is deterministic but carries no lexicographic meaning, and no part of the
language promises one.

[image.md](image.md) § "Stable symbol identity" owns the argument for why the
image work needs this property.

## The display memo

Identity needs no table. Display does, because a hash cannot be printed as a
name — and display is now the only thing a symbol table is for.

`SymbolTable` is that table: one hash → spelling map per instance, serving
both vocabularies. A symbol and a keyword with the same spelling share one
entry — the map's domain is spellings, not values, and the tag that separates
`map` from `:map` lives in the `Value`, not in the memo. It takes no lock,
because it is not shared, and it is dropped with the instance that owns it, so
no name outlives the run that learned it.

- `intern(&mut self, name)` hashes, records the spelling, and returns the
  `SymbolId`.
- `name(&self, id)` returns the recorded spelling, or `None`.
- `keyword(&mut self, name)` and `keyword_name(&self, hash)` are the keyword
  entry points onto the same map, keyed by the raw hash (a keyword payload is
  not a `SymbolId`).
- `SymbolId::of(name)` / `keyword_hash(name)` hash without recording, and need
  no table at all. Both are `const fn`, so a well-known id can be a constant.

A memo learns a name where names already travel, and nowhere else:

| Site | What arrives |
|------|--------------|
| the reader | every identifier and keyword token in the source text |
| primitives | every keyword a native mints through its `NativeCtx` |
| the plugin ABI | every keyword a plugin mints through `make_keyword`'s call ctx |
| `send` | the bundle's name table (symbols, named from the sender's memo) and the inline names keywords already carry, replayed into the receiver's memo |
| hydration | the image's name table, replayed into the hydrating instance |
| `ConstTemplate` decode | symbols and keywords encoded by name in the constant pool |

Because a name is recorded only at those sites, a memo answers for the names
its instance has actually met. That is the whole contract: a value whose name
was never recorded is still a perfectly good value, and still compares
correctly. It just has no name to print.

## Reading a name, and not reading one

Identity questions never touch the memo. Asking whether a binding is `count` is
`bi.name == SymbolId::of("count")` — one integer compare against a constant,
with no table in scope and no allocation. Every such site in the compiler is
written that way; a lookup that returns a `&str` only to compare it against a
literal is the shape to avoid.

Display threads the memo explicitly. `Value::display_with(Option<&SymbolTable>)`
and `Value::debug_with` carry it into the formatter; the bare `Display` and
`Debug` impls cannot, because `fmt` takes only `&self` and a formatter. A symbol
formatted without a memo renders `#<symbol:hash>`; a keyword renders
`#<keyword:hash>`. With a memo they render the bare name and `:name`
respectively (`map`, not `'map` — the quote is reader syntax for `quote`, not
part of the printed form).

The unresolved forms are deliberately not keyword or symbol spellings: any
rendering that parses as a literal (`:0xcbf2…`, `:unknown`) denotes a real
value — the wrong one. `#<…>` is the codebase's marker for an object the
printer cannot render faithfully, and here it doubles as a canary: every mint
site is a learning site, so an unresolved form in user-facing output points at
a missed one, or at a formatter that failed to thread the memo.

## Collisions are fatal

Two different names that hash alike would make two names one name, silently and
everywhere at once. So every learning site is also the collision check:
recording a hash the memo already maps to a different name panics. Nothing
recovers — a collision is a property of the program's vocabulary, not of one
execution, so retrying or falling back would only move the corruption.

Checking at the learning site rather than in one process-wide table is what
extends the check past this process. A name arriving from a dump or from another
build is checked against the receiving instance's names as it lands, which is
the moment it would first confuse a reader.

The payload is a full `u64` — the tag lives in the `Value`'s separate tag word,
so all 64 bits carry discrimination. For a program with 10,000 distinct names
the birthday bound puts the collision probability near 3 in 10^12. That bound
covers names built at run time; the built-in vocabulary is proven rather than
estimated, by a test that hashes every static name — the primitive tables and
their aliases, and every symbol read from core, prelude, and stdlib — and
asserts they are distinct, so a build whose own vocabulary collides fails CI.

`SymbolId::SYNTHETIC` (`u64::MAX`) marks a compiler-generated binding with no
source-level name — a phi temporary, a desugaring's scratch variable. It is
reserved: `intern` panics if a real name hashes onto it, so the sentinel can
never be mistaken for a symbol somebody wrote.

Because the memo is one map, the collision check spans both vocabularies: a
keyword spelling colliding with a symbol spelling is caught by the same guard,
at whichever learning site meets the second spelling first.

## Keywords in the plugin ABI

A stable-ABI plugin mints a keyword through `make_keyword`, which — like every
other constructor in the ABI — takes the per-call `CallCtx`. The ctx carries
the owning instance's memo, so a plugin keyword is learned by the instance the
call belongs to, and `keyword_name`/`as_keyword_name` read back through the
same ctx. `intern_keyword` is a pure hash: it records nothing and needs no
ctx. `Value::keyword(&str)` in the host is equally pure — construction is
identity only, and naming happens at the learning sites above.

## Where the id appears

| Site | Form |
|------|------|
| `Value` | `Value::symbol(SymbolId)`, `as_symbol() -> Option<SymbolId>`; `Value::keyword(&str)`, `keyword_hash() -> Option<u64>` |
| Struct and set keys | `TableKey::Symbol(SymbolId)` and `TableKey::Keyword(u64)`, ordered by hash |
| HIR bindings | `BindingInner::name`, `SymbolId::SYNTHETIC` for temporaries |
| LIR constants | `LirConst::Symbol(SymbolId)`, `LirConst::Keyword(u64)` |
| Bytecode | none directly — a symbol reaches bytecode as a constant-pool `Value` (a `u16` pool index) or inside a `ConstTemplate`, which encodes symbols by name |

`Value::symbol` takes a `SymbolId`, not a bare `u64`, because a keyword hash is
also a `u64`: the newtype is what stops `Value::symbol(keyword_hash(s))` from
compiling.
