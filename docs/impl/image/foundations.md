# Foundations

<!-- audited: 2026-09-22 -->

Five representation fixes the image needs: each pays at runtime today, and each
deletes image machinery.

The image effort does not start with images. Each fix below lands green
against the existing corpus with no image machinery, and **deletes** image
machinery that would otherwise have to be built and then thrown away. Building
the image first would mean shipping remap passes, re-sort passes, and the
syntax and LIR codecs whose only purpose is to compensate for representations
we intend to fix anyway.

[image.md](../image.md) owns the design these four serve, and
[plan.md](plan.md) records the order they landed in.

## Stable symbol identity — landed

A `SymbolId` used to be a dense per-table index minted in first-intern order —
a process-local accident. Everything downstream compensated: every code object
carried a `symbol_names` map for cross-table remap, `send` re-interned by name,
`CompileCtx` leaned on a fragile "registration order is deterministic"
invariant, and — the sharpest edge — immutable structs and sets are *sorted
arrays* whose order (`TableKey`/`Value` compare symbols by raw id) was
process-local, so a persisted sorted container was correct only where it was
built.

A `SymbolId` is now the 64-bit FNV-1a hash of the name, and a name lives in a
per-instance display memo rather than a process-wide table
([symbol.md](../symbol.md) owns the model). Symbol values are as portable as
keyword values, and sort orders are stable by construction, so the image needs
no symbol remap pass, no re-sort pass, and no symbol watermark.

Identity comes free; display does not. A hydrating instance holds none of the
dump's names, which is what the name table is for, and replaying it is also
where a cross-build collision is caught.

## Region-native immutable structs — landed

`LStruct` used to hold `Vec<(TableKey, Value)>`, and two `TableKey` arms owned
Rust heap memory: `String` owned a `String` and `Array` owned a
`Vec<TableKey>`. The payload is now `RegionSlice<(TableKey, Value)>` and both
key arms hold a `Value` pointing at a region-resident string or array, as
arrays and strings already did. `TableKey` is `Copy` and sealed;
[values.md](../values.md) § "Struct keys" owns the key model — a borrowed probe
key, an interned stored key, and an owning `SendKey` for `send` and the disk
cache. Payoff now: immutable structs allocate nothing on the Rust heap, and a
`get` probe builds its key without allocating. Payoff for images: structs are
body data, and a struct's key bytes are self-edges of its own region.

## Region-native closure templates — landed

`ClosureTemplate` was ~20 `Rc`/`Vec` fields, and `MakeClosure` cloned the whole
blueprint into the instance region on every closure creation — 13 refcount
bumps and two Rust-heap allocations apiece, in a `HeapObject` variant whose 288
bytes set the size of every other variant.

A code object is now three things
([region/template.md](../region/template.md) owns the argument): a compile-time
blueprint, a `CodePayload` holding every variable-length field inline in region
pages, and a two-word region-resident header naming that payload. The payload
is materialized once per blueprint and shared, so `MakeClosure` copies two
words and takes one cross-region reference rather than copying a function's
bytecode per iteration of a loop that builds a closure. Payoff for images: the
payload is body data — bytecode, constants, name and doc as region strings,
masks and release tables as inline slices, and source locations as a sorted
`RegionSlice<LocEntry>` over an interned file table, replacing a
`HashMap<usize, SourceLoc>` whose `String` file names could not be sealed at
any price.

The header keeps one `Rc` to its blueprint, for the four questions the payload
cannot yet answer: the nested-lambda blueprints a `MakeClosure` indexes, the
LIR the JIT promotes from, the defining syntax, and the SPIR-V cache. The
syntax foundation removed one; the image milestone's own dump removed the
second by making child templates body data; the third is the GPU cache the
design drops. The LIR foundation below is the last of them, and it takes the
`Rc` with it.

## Region-native syntax — landed

Syntax is required for images, not a reconstruction nicety: a macro *is* its
template (`MacroDef.template: Syntax` plus parameter lists; the compiled
transformer is a lazily filled cache), so the expander cannot be rebuilt
without syntax. A Rust-heap `Box` tree would force a side-stream codec and a
lazy-decode seam — a serializer whose entire job is to work around the
representation.

Syntax is a region-native immutable tree instead: nodes and child slices
inline in region pages, `Copy` POD with no `Drop`, spans and hygiene scopes as
plain fields ([syntax.md](../syntax.md) owns the model). Macro templates, the
syntax a `Value` wraps, and inline-fn syntax are body data, demand-paged like
everything else, and `HeapObject::Syntax` owns its tree inline rather than
through a `Box` no image could seal around.

The tree is region-native everywhere, not only at the value boundary — the
expander's working tree included. A mutable Rust tree in the compiler beside a
region-native form at the boundary would be two representations of one datum:
conversion seams, double maintenance, and a standing invitation for the two to
drift. The bar is **parity**, since expansion is compile-path-hot and the
fallback compile pays it too; [measurements.md](measurements.md) item 5
measured the prototype at 2–5× faster than a Rust-heap tree on every
expansion-hot operation, so the boundary-only split never had a case.

Hygiene scope ids minted by the expander remain process-local counters; the
image records a scope watermark so a fresh expander mints above every scope
baked into persisted syntax.

## Region-native LIR — to land

The JIT compiles from `lir_function`: a Rust-heap `LirFunction` hanging off the
blueprint every code object still carries. It is the last of the four questions
that blueprint answers (§ "Region-native closure templates"), so it is what
keeps `TemplateProto` alive — and `TemplateProto` is a second copy of the
bytecode, the constants, the masks and the region tables the payload already
holds region-natively.

Four things the port buys. None is a compile-time number;
[measurements.md](measurements.md) item 7 measured those, and they are real but
small.

- **Save and load become the mechanism the image already has.** Body-resident
  LIR dumps in the dumper's own walk and arrives with the mapping. The
  alternative is an encoded side-stream: a second encoder, a per-function
  decode, and a lazy seam — the shape the syntax foundation exists to delete.
- **One representation.** The port retires `TemplateProto`, `send`'s LIR codec
  (`convert_value_consts_for_send`, `LirConst::ClosureRef`, `LirConst::ValueRef`
  and the value pool behind them), and the stream. A code object becomes region
  data end to end.
- **One portability rule.** Sealed region data crosses a worker, an image and a
  socket the way every other value does. The hand-written `Send` claim on
  `JitTask` becomes a property of the type instead of a comment.
- **Allocation the project can see.** Building the boot sources' LIR costs
  21,281 `malloc` calls, which no gauge the region system owns can see. As
  region pages they answer to `--region-page-size`, `--page-pool-max`,
  `arena/page-claims`, the leak suite, `--trace=scrub` and `--trace=guardfree`.

Two things stay work rather than argument. The JIT worker runs on another
thread and a region belongs to one `RegionStore`, so a promotion still copies
its function out — 8.7 ns an instruction against the Rust clone's 13.8 ns, but
a copy either way. And the passes that rewrite LIR in place also resize it,
which a fixed-extent slice turns into build-then-materialize. Syntax met that
wall and answered it by copying as it stamps ([syntax.md](../syntax.md)); the
better answer is a slice that grows in its own region, which no foundation has
needed yet.
