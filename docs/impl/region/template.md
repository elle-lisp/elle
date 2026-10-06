# Code objects — a payload, a header and a code unit

<!-- audited: 2026-10-06 -->

A code object is a payload in a code region and a one-word header naming it, and a compile unit owns the region.

A closure template is the code object of one lambda: its bytecode, constant
pool, source locations, LIR, and the region tables its body needs. This doc owns
the argument for how that object is represented and who owns each part. The
foundation it serves is named in [image/foundations.md](../image/foundations.md);
the rule it obeys is [model.md](model.md) § "Constants lower as ordinary
allocations".

## Two things, not one

- **`CodePayload`** — the *payload*: every field of the code object, inline in
  region pages. The emitter writes it once, at emission.
- **`ClosureTemplate`** — the *header*, the thing `HeapObject::ClosureTemplate`
  holds. It is one `RegionSlice<CodePayload>` of length one, and nothing else.

A closure instance references a header, and a header references a payload.
`MakeClosure` builds a header, not a payload — that is the point of the split.
No part of a code object is Rust-heap data, so the header and the payload are
sealed data an image carries as they stand ([sealing.md](../image/sealing.md)).

## A compile unit owns one code region

Every compile mints one **code region** on its heap, and names it with a
`CodeArena`: a heap and a region, `Copy`, like a `SyntaxArena`
([syntax.md](../syntax.md)). The emitter writes each payload into it at
emission:

1. A nested lambda's payload is written at the `MakeClosure` that builds the
   lambda, and a header over it is allocated beside it.
2. That header goes into the parent's **child table**, at the index the
   `MakeClosure` instruction carries.
3. The entry function's payload is written last, with its own header. Its
   child table holds the headers of the lambdas the entry builds.

The result is a **`CodeUnit`**: the entry header, and one counted reference to
the code region. `Bytecode` is the emitter's working buffer and nothing more;
the pipeline hands out a `CodeUnit`.

Packing a whole unit into one region is the shape the old payload cache reached
by accident of timing. The unit is the thing that has one lifetime, so the
unit now owns the region outright, and no cache stands between them.

## How long a code region lives

A code region is an ordinary counted region. Four kinds of holder take a
reference to it:

| Holder | Reference |
|--------|-----------|
| a `CodeUnit` | one, taken at the compile and released when the handle drops |
| a header in another region | a counted cross-region edge, recorded at its allocation |
| a JIT or SPIR-V cache entry | one, through a `CodePin` ([jit.md](../jit.md)) |
| a header or payload inside the code region | none: a self-edge |

So a dropped unit's region frees when the last header built from it is freed.
Nothing about a code object needs a second reclamation mechanism, and no unit
is a process root. The standard library's unit and a REPL line's unit are
released after their runs like any other. The exports and the bindings they
leave behind are closures, and each closure's header holds the region.

A `CodeUnit` is Rust-held, so it is RAII: cloning it takes a reference and
dropping it releases one. A unit must not outlive the heap its region lives on.
A debug build counts the live handles and checks the count at teardown.

## Every reference held from Rust is named on the heap

A macro expansion reclaims its scratch by balancing the references a scan of
heap contents cannot explain ([rules.md](rules.md) § "Macro expansion — a
closed allocation scope"). A `CodeUnit` and a `CodePin` hold their references in
Rust, where that scan cannot reach them. So the heap keeps a **code-hold
registry**: every code region a Rust handle holds, with the number of handles.
The reclaim excludes those regions, exactly as it excludes the process roots.

The registry is also what the teardown check reads, and the counter-factual for
it is a unit compiled inside an expansion that outlives it. The reclaim would
take the unit's reference, and the unit's own release would then free a region
whose headers still read it.

## A unit runs on the heap it was compiled on

A payload is region data and belongs to one heap. An instance compiles on the
heap its program runs on, but a standalone `CompileCtx` runs its macro VM on a
heap of its own, and so does the embedding shape built on one
([heaps.rs](../../../src/runtime/tests/heaps.rs)). Running a unit there would
leave its payloads in regions the executing heap does not own: no edge the
executing heap records would keep them, and no teardown of it would release
them.

So `VM::execute` and `VM::execute_scheduled` copy a unit compiled on another
heap into a code region of the executing heap first, and run the copy. The
copy keeps every payload's sharing, rewrites each child table to the copied
headers, and copies constant and LIR values as they stand. A constant that
names a value on the compile heap stays a foreign reference, which the alloc
scan's ownership test already skips.

## One constructor builds a nested lambda's payload

The emitter holds two inputs at a `MakeClosure`: the lambda's frozen LIR, and
the bytecode its own emission produced. `PayloadParts::lambda` takes those two
and the capture count the site decided, and fills every field. The two release
tables are the fields that cost the most when one is left out. A closure built
from such a payload runs correctly until one of its frames is abandoned. The
error exit then walks an empty table, and every region that frame owed is
stranded ([mechanism.md](mechanism.md) § "An abandoned frame runs the releases
it still owes").

The JIT builds no payload. It refuses a function that holds a `MakeClosure`,
and it has no translation for one.

Pinned by
`lir::emit::tests::a_nested_lambdas_payload_carries_the_frame_release_tables`.

## The WASM backend copies the module's own payload

`rt_make_closure` is the host function an emitted module calls at every closure
creation. It has no LIR to read. What it holds is the module's dual-compiled
code unit, whose payload for that closure was written from the closure's LIR
([wasm.md](../wasm.md) § "Cross-thread spawn"). The call also passes the shape
of the frame the lambda runs in — arity, the three counts, the two capture
masks, the signal — through linear memory.

`PayloadParts::wasm_closure` takes the two. The **code half** comes off the
module's payload whole; the **shape half** comes off the call, and so does the
dispatch index. The release tables, the location table, the merge set and the
child table are in the code half, so none of them is the site's to remember.

The dual-compiled bytecode is what a spawned OS-thread worker runs, so a frame
of it abandoned on an error exit walks whichever table this constructor
carried.

Pinned by
`wasm::tests::closure::a_wasm_built_closure_carries_the_frame_release_tables`
and `..._carries_the_locations_and_the_merge_set_its_body_names`.

## Why the payload is shared and the header is not

`MakeClosure` runs once per closure *creation*, which for a closure built in a
loop is once per iteration. Whatever it copies, it copies that often.

The header is per-creation because the region model says so: a heap literal is
an ordinary, reclaimable allocation born in the executing frame's region
(model.md § "Constants lower as ordinary allocations"), and a closure template
is a heap literal. The payload is not per-creation, because copying a
function's whole bytecode on every iteration of a loop that builds a closure is
a cost the old blueprint did not have.

So the header copies the child's payload slice out of the parent's child table,
and building one copies one word and takes one cross-region reference. The
rejected alternatives:

- **Copy the payload per creation.** Every region stays self-contained, with no
  cross-region edge — and every closure creation copies the function's bytecode,
  constants and LIR. It trades a bounded win for an unbounded loss on exactly
  the shape (a closure in a loop) that the region model exists to make cheap.
- **Hand out the child table's own header.** No per-creation allocation at all,
  but it makes the instance-to-template edge a cross-region edge on every
  closure, and the header's lifetime the unit's rather than the frame's — the
  promotion Rule 3 forbids.

## What the payload holds

Every field is inline in region pages. Nothing in a `CodePayload` owns Rust
heap memory, so the object's bytes *are* the object — the sealing property
[sealing.md](../image/sealing.md) requires of body data.

One field holds a process-local number inside those bytes: the origin span's
file id indexes a process-wide interner, exactly as a syntax node's does, so an
image rewrites it from the file table ([format.md](../image/format.md)).

| Field | Representation |
|-------|----------------|
| bytecode | `RegionSlice<u8>` |
| constants | `RegionSlice<Value>` |
| locations | `RegionSlice<LocEntry>`, ascending by bytecode offset |
| files | `RegionSlice<RegionSlice<u8>>` — the interned file names `LocEntry` indexes |
| name, doc | `RegionSlice<u8>` |
| region table | `RegionSlice<StaticRegion>` |
| merged slots | `RegionSlice<u32>`, ascending |
| frame-release slots / regions | `RegionSlice<u16>` / `RegionSlice<u32>`, ascending |
| capture-locals mask | `RegionSlice<u64>` — the mask's words, unbounded in width |
| strict-struct keys | `RegionSlice<RegionSlice<u8>>` — the `&named` key set |
| children | `RegionSlice<Value>` — the headers a `MakeClosure` indexes, in instruction order |
| origin | a `Span` and a present flag — where the lambda was written, for `meta/origin` |
| lir | a `LirBody` and a present flag — the frozen function the JIT promotes from ([lir.md](../lir.md)) |
| arity, param and local counts, signal, capture-params mask, vararg kind, rest-list layout ([restlist.md](restlist.md)), WASM index | scalars, inline |

The LIR body is present on the payload of every nested lambda, and absent on an
entry function's and on a placeholder's. The body holds only what LIR alone
knows. Every header field the two share — the merge set and the two release
tables among them — is the payload's, and a view over the body reads it there,
off the one record. Freezing records those three tables ascending, so the
payload's copy and a `LirCode`'s agree on order as well as content.

Two of the fields changed shape rather than merely moving.

**Source locations are a sorted table, not a hash map.** A `LocationMap` was
`HashMap<usize, SourceLoc>` and a `SourceLoc` owned a `String` file name — two
Rust-heap owners per entry, and a hash map is not byte-self-contained at any
price. A `LocEntry` is four `u32`s (bytecode offset, file index, line, column)
and the file names are interned once per payload, so a lookup is a binary
search over a flat slice. The table is ascending by offset, which also makes
`display_label`'s "smallest-offset location" the first entry instead of a scan.

**The merged-slot set is a sorted slice, not a hash set.** Membership is a
binary search. The set is empty unless a builder-idiom merge fired
([merging.md](merging.md)), so the common case is a length check.

## The placeholder is a payload in the root region

A fiber that runs no bytecode still names a code object — the root fiber, whose
execution context is top-level bytecode rather than a closure, and a
native-iterator fiber, which the resume path short-circuits. The instance
writes one placeholder payload, a nullary body of a single `Return`, into its
pinned root region, and every such fiber shares its header. The root region is
a process root, so the placeholder lives as long as the instance.

## The executing context is the header

`Code` — what the dispatch loop, the tail-call trampoline, and every suspended
frame thread as the template-derived half of the execution context — is the
header plus nothing. Bytecode, constants, locations, the merge set, the two
release tables, the reserved-local count and the child table all come from the
payload. So `Code` wraps a `ClosureTemplate` and adds no fields of its own, and
swapping the executing code object on a tail call copies one word.

A `Code` takes no reference, so a payload must outlive every activation that
runs it. A closure's body runs while some header over its payload is live: its
own closure's, or a sibling's from the same unit. An entry function runs while
its caller holds the unit. A debug build checks the payload's region at every
body entry, so an activation that outlives its code fails there rather than
reading recycled pages.
