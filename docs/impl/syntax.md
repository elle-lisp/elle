# Syntax — a region-native immutable tree

The pre-analysis tree the reader produces, the expander rewrites, and the
analyzer consumes. Every node, every child slice, and every string payload
lives in region pages. A node is plain old data: `Copy`, 64 bytes, with no
`Box`, no `Vec`, no `Rc`, and no `Drop`.

[image.md](image.md) § "Region-native syntax" owns the argument for why the
image work needs this property. This document owns the model.

## The node

```rust
pub struct Syntax {
    pub kind: SyntaxKind,        // 24 bytes: tag + one 16-byte payload
    pub span: Span,              // 20 bytes: four u32 offsets + a FileId
    scopes: RegionSlice<ScopeId>,// 16 bytes
    pub scope_exempt: bool,
}
```

`SyntaxKind` is `Copy`. Every payload is a region handle, never an owned
allocation:

| Variant group | Payload | Derefs to |
|---------------|---------|-----------|
| `Symbol`, `Keyword`, `String`, `StringMut` | `RegionStr` | `&str` |
| `List`, `Array`, `ArrayMut`, `Struct`, `StructMut`, `Set`, `SetMut`, `Bytes`, `BytesMut` | `RegionSlice<Syntax>` | `&[Syntax]` |
| `Quote`, `Quasiquote`, `Unquote`, `UnquoteSplicing`, `Splice`, `SyntaxLiteral` | `SynRef` | `&Syntax` |
| `Nil`, `Bool`, `Int`, `Float` | immediate | — |

Every payload dereferences to an ordinary Rust view, so a reader of the tree
matches and walks it without knowing about regions: `SyntaxKind::List(items)`
indexes, iterates, and slices as `&[Syntax]`, and `as_symbol()` answers
`Option<&str>`.

Children are stored **by value, inline** in one slice, not as a slice of
pointers. A list of *n* elements is one allocation of *n* nodes; walking it
touches contiguous bytes.

### Why a name and not a hash

A symbol node carries its spelling as a region string, not a `SymbolId`.
Identity would be cheaper as a hash, but three consumers need the spelling
where no `SymbolTable` is in scope: the formatter reproduces source text, the
reader's error messages name the token, and the epoch rewriter matches and
emits names. The node is 64 bytes either way — the largest payload is the
16-byte `RegionSlice`, which a hash cannot shrink — so the spelling is free.

Identity comparisons are short-string compares against a literal —
`is_symbol("def")` — which is what the analyzer's hot paths ask for.

## Where a node lives

Nodes are region data, so every constructor names a region. `SyntaxArena` is
that name: a `Copy` handle over a heap and a region on it.

```rust
let node = Syntax::list(&arena, &items, span);
```

An instance has two syntax regions, and a node belongs to exactly one:

- The **working** arena is one transient region per compilation unit, minted
  by `pipeline::compile`'s `with_syntax_arena` and freed when the unit's
  bytecode is built. The parsed tree, every intermediate expansion, and the
  expanded tree live here.
- The **template** arena is one region per instance, registered as a process
  root by `CompileCtx` and released at teardown. Macro templates live here,
  because `MacroDef` outlives the compilation unit that defined it.

One rule keeps every pointer valid: **a node may point into its own arena or
into the template arena, and nowhere else.** The template arena is a process
root, so it outlives every working arena on the instance; the reverse edge
never exists, because `handle_defmacro` copies a template out of the working
arena as it registers the macro.

A caller with no runtime — `elle fmt`, the pre-VM `unicode!` scan, the epoch
rewriter's template parse — builds a `SyntaxHeap`, which owns a bare
`FiberHeap` and one region on it, and hands out an arena.

### A syntax `Value` owns its tree

`HeapObject::Syntax` holds its node inline, and `value::build::syntax`
**copies** the tree into the value's own region as it builds the object. The
copy is what makes the object self-contained: no cross-region edge to record,
nothing to keep alive but the value's own region, and page bytes that are the
whole object — which is what an image needs to dump it.

This is the rule `RegionSlice`'s module docs state for every slice-backed
clone — copy the payload into the clone's own region — applied to syntax.
Every builder of a syntax object obeys it: `value::build::syntax`,
`with-traits`, and `ConstTemplate::SyntaxSymbol`'s materializer.
`Syntax::from_value` copies the other way, out of the value's region into the
working arena, which is why a macro transformer's entire output survives the
reclaim of its allocation scope.

## Mutation, sharing, and the stamped copy

The tree is immutable data shared by pointer. `Syntax` is `Copy`, so passing a
subtree, storing it in two places, or handing a macro template to a hundred
call sites costs 64 bytes and no walk.

Two operations do change a tree, and both take an arena because both build a
new one:

- `stamp_scope` adds a scope to every non-exempt node.
- `flip_scope_recursive` flips one scope on every non-exempt node — the
  hygiene operation applied to a transformer's result.

Both copy: a shared subtree must not see a scope its other holders did not ask
for. [image.md](image.md) risk item 5 measures the copy at 37 ns per node,
which is the budget the whole design was checked against.

In-place mutation stays legal on a **uniquely owned** working tree, through
`Syntax::children_mut`. The expander uses it where it has just built the
subtree and no other holder exists. The discipline is the caller's: the
accessor names it, and `a_stamped_copy_does_not_disturb_its_source` pins the
case that gets it wrong.

## Scopes

`scopes` is a `RegionSlice<ScopeId>` with no inline capacity: `add_scope`
allocates a slice one longer and writes the new set. Measured scope sets are
tiny — every one of the ~430,000 scope operations in a stdlib expansion ended
with three scopes or fewer — and regrowing on each add costs 8 ns per visit,
so the growth is not on any path that matters.

## Span

`Span` is `Copy` POD: `start`, `end`, `line`, `col` as `u32`, and a `FileId`.

The file name is an index into a process-wide interner (`syntax::files`), not
a `String`: a span rides inside every syntax node and every HIR and LIR node,
so an owned name would be one Rust-heap allocation per source location, cloned
on every merge. `FileId::NONE` is the absent file, so the `Option` is out of
the representation but not out of the API — `Span::file()` answers
`Option<&'static str>`.

`Span` crosses process boundaries inside serialized LIR (the stdlib cache and
`send`), where a `FileId` means nothing. Its `Serialize` writes the *name* and
its `Deserialize` re-interns, so an id is never the thing that travels.

## Crossing a thread

A region belongs to one `RegionStore`, so a tree cannot cross a thread as
pointers. `send` carries a `SendSyntax` mirror instead: the sender owns the
strings and children on the way out, and the receiver rebuilds them in its own
arena on the way in — the same trade a string makes. Every kind crosses,
`SyntaxLiteral` included.

## Where a lambda's source location lives

`ClosureTemplate.origin` is an `Option<Span>` — 20 bytes of POD, read by
`(meta/origin f)` for a file, a line, and a column. A closure template holds no
syntax tree: nothing has ever read one off it, and a retained tree would pin a
compile-time arena for the life of every compiled closure.

## Pinned by

- `src/syntax/tests/region.rs` — the node is `Copy` POD at 64 bytes; children
  and name bytes resolve to the arena's region; payloads deref to `&str`,
  `&[Syntax]`, and `&Syntax`; a copy into another arena shares nothing; a
  stamped copy does not disturb its source; scope sets grow without loss; a
  syntax `Value` owns its tree.
- `src/syntax/span/tests.rs` — `Span` is 20-byte `Copy` POD, its file name
  interns, and serde carries the name rather than the id.
- `src/syntax/files/tests.rs` — interning is idempotent and the empty name is
  the absent name.
- `src/value/send/tests.rs` — a tree round-trips through the mirror with
  spans and scopes intact, `SyntaxLiteral` included.
- `tests/integration/syntax_regions.rs` — a compiled unit's syntax arena is
  released with the unit (counter-factual: leaving it live leaks one region
  per compile), a macro template outlives the unit that defined it, and a
  `SyntaxHeap` reads and reclaims with no runtime in reach.
