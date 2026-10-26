# Foundations

<!-- audited: 2026-10-23 -->

Six representation fixes the image rests on: each pays at runtime, and each
deletes image machinery.

The image effort does not start with images. Each fix below lands green
against the existing corpus with no image machinery, and **deletes** image
machinery that would otherwise have to be built and then thrown away. Building
the image first would mean shipping remap passes, re-sort passes, and the
syntax and LIR codecs whose only purpose is to compensate for representations
we intend to fix anyway.

[image.md](../image.md) owns the design these six serve, and
[plan.md](plan.md) records the order they land in. All six have landed.

## Stable symbol identity — landed

A `SymbolId` used to be a dense per-table index minted in first-intern order —
a process-local accident. Everything downstream compensated. Every code object
carried a `symbol_names` map for cross-table remap, `send` re-interned by name,
and `CompileCtx` leaned on a fragile "registration order is deterministic"
invariant. The sharpest edge: immutable structs and sets are *sorted arrays*,
and their order (`TableKey`/`Value` compare symbols by raw id) was
process-local. So a persisted sorted container was correct only where it was
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
[values.md](../values.md) owns the key model — a borrowed probe
key, an interned stored key, and an owning `SendKey` for `send` and the disk
cache. Payoff now: immutable structs allocate nothing on the Rust heap, and a
`get` probe builds its key without allocating. Payoff for images: structs are
body data, and a struct's key bytes are self-edges of its own region.

## Region-native closure templates — landed

`ClosureTemplate` was ~20 `Rc`/`Vec` fields, and `MakeClosure` cloned the whole
blueprint into the instance region on every closure creation. Each clone cost
13 refcount bumps and two Rust-heap allocations, in a `HeapObject` variant whose
288 bytes set the size of every other variant.

A code object is now two things
([region/template.md](../region/template.md) owns the argument): a
`CodePayload` holding every field inline in region pages, and a one-word
region-resident header naming that payload. The emitter writes a compile
unit's payloads into one code region. So `MakeClosure` copies one word and
takes one cross-region reference, where it used to copy a function's bytecode
per iteration of a loop that builds a closure. Payoff for images: the payload
is body data. Bytecode, constants, name and doc are region strings, and masks
and release tables are inline slices. Source locations are a sorted
`RegionSlice<LocEntry>` over an interned file table, replacing a
`HashMap<usize, SourceLoc>` whose `String` file names could not be sealed at
any price.

The header used to keep an `Rc` to a compile-time blueprint, for four questions
the payload could not answer. They were the nested lambdas a `MakeClosure`
indexes, the LIR the JIT promotes from, the defining syntax, and the SPIR-V
cache. The
syntax foundation removed one, the child table another, and the SPIR-V cache
moved to the VM. The LIR foundation below answered the last from the payload
and deleted the blueprint.

## Region-native syntax — landed

Syntax is required for images, not a reconstruction nicety: a macro *is* its
template (`MacroDef.template: Syntax` plus parameter lists; the compiled
transformer is a lazily filled cache), so the expander cannot be rebuilt
without syntax. A Rust-heap `Box` tree would force a side-stream codec and a
lazy-decode seam — a serializer whose entire job is to work around the
representation.

Syntax is a region-native immutable tree instead: nodes and child slices
inline in region pages, `Copy` POD with no `Drop`, spans and hygiene scopes as
plain fields ([syntax.md](../syntax.md) owns the model). Macro templates and
the syntax a `Value` wraps are body data, demand-paged like everything else,
and `HeapObject::Syntax` owns its tree inline rather than through a `Box` no
image could seal around.

The tree is region-native everywhere, not only at the value boundary — the
expander's working tree included. A mutable Rust tree in the compiler beside a
region-native form at the boundary would be two representations of one datum:
conversion seams, double maintenance, and a standing invitation for the two to
drift. The bar is **parity**, since expansion is compile-path-hot and the
fallback compile pays it too. The region-native tree is faster than a Rust-heap
tree on every expansion-hot operation, so the boundary-only split never had a
case.

Hygiene scope ids minted by the expander remain process-local counters; the
image records a scope watermark so a fresh expander mints above every scope
baked into persisted syntax.

## Region-native LIR — landed

A code payload carries its function's LIR as region-native records, and the
JIT, the other backends, `send`, introspection and the image all read it there
([lir.md](../lir.md)). The emitter writes each payload into its compile unit's
code region, so no compile-time blueprint holds a second copy. The lowerer
builds the same records in a working region from the start, so no Rust-heap
form of LIR is left.

Four things the port buys. The compile-time gains are real but small, and none
of the four is one of them.

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
- **Allocation the project can see.** A Rust-heap working form allocates
  through `malloc`, where no gauge the region system owns can see it. Built in
  a working region, the LIR answers to `--region-page-size`, `--page-pool-max`,
  `arena/page-claims`, the leak suite, `--trace=scrub` and `--trace=guardfree`.

Two things stayed work rather than argument. The JIT worker runs on another
thread and a region belongs to one `RegionStore`, so a promotion still copies
its function out, faster than the Rust clone did, but a copy either way. And
the lowerer splices into blocks it has already finished, which a fixed-extent
slice cannot take. Syntax met that wall and answered it by copying as it stamps
([syntax.md](../syntax.md)). LIR answers it with a slice that grows in its own
region, `RegionVec`.

### The shape

[lir.md](../lir.md) owns the node's details. An instruction is a 48-byte
`repr(C)` record with no implicit padding, and its variable-length operands sit
in one pool per function, named by index. A function refers to its
own parts by index everywhere, so only its top-level slices are pointers. An
instruction page therefore carries no relocation slot and stays clean in an
image. A reader decodes a record into a borrowed `InstrRef` with safe Rust over
slices, so a corrupt index panics where it is read and the verifier need not
fault in LIR pages to bound one.

### Landing

The port lands as a seam and then three stages, each green on its own:

1. **One read form** — landed. Freezing turns a lowered function into the
   records, and every reader except the lowerer reads them through `LirView`.
   `send`'s LIR codec is deleted, because a frozen function carries its values
   in a table that crosses through the ordinary value walk.
2. **LIR in the payload** — landed. The code payload carries the records as
   body data, so a closure hydrated from an image reaches the JIT. This is the
   image's goal.
3. **Retire `TemplateProto`** — landed. A code object is one payload slice in
   its compile unit's code region. The payload cache, the blueprint arm of every
   header and the second copy of the bytecode are gone.
4. **A region-native lowerer** — landed. The lowerer builds the records in a
   working region, through slices that grow, and freezes each function as it
   finishes. `LirInstr`, `LirFunction`, `BasicBlock`, `SpannedInstr`,
   `LirModule` and `LirConst` are gone: `InstrRef` is the one instruction type,
   built by the lowerer and decoded by every reader.

Each stage's tests carry the pins it landed with, and [plan.md](plan.md) records
the acceptance gate the stages share.

## Region-native mutable stores — landed

No image can seal a mutable container that keeps its contents on the Rust heap,
behind an `Rc` and a `RefCell`, as every mutable container used to. Each one
now keeps its contents in an extent of its own region, or a box or a capture
cell holds its value inside its shell, so a mutable value is body data. The fix
deletes the environment image's side-stream before anybody writes one.
[Mutable stores in region pages](../mutable.md) owns the design, and
[sending a region](../send.md) rests on it too.
