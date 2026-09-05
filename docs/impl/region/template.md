# Code objects — a blueprint, a payload, and a header

<!-- audited: 2026-09-05 -->

A closure template is the code object of one lambda: its bytecode, constant
pool, source locations, and the region tables its body needs. This doc owns the
argument for how that object is represented and who owns each part. The
foundation it serves is named in [image.md](../image.md) § "Region-native
closure templates"; the rule it obeys is
[model.md](model.md) § "Constants lower as ordinary allocations".

## Three things, not one

The code object is split into three, because the three have different owners
and different lifetimes:

- **`TemplateProto`** — the compile-time *blueprint*. Plain Rust data the
  emitter builds, owned by whatever holds the compiled program: a `Bytecode`'s
  `child_protos`, a JIT code object, a `Code`. It never enters a region.
- **`CodePayload`** — the *payload*: every variable-length field of the code
  object, flattened into region pages. One payload per blueprint, materialized
  once per heap and shared by every header built from that blueprint.
- **`ClosureTemplate`** — the region-resident *header*, the thing
  `HeapObject::ClosureTemplate` holds. It is two words: a `RegionSlice` naming
  its payload, and an `Rc` to the blueprint it came from.

A closure instance references a header; a header references a payload; a
blueprint owns the right to materialize more headers. `MakeClosure` builds a
header, not a payload — that is the whole point of the split.

## Why the payload is shared and the header is not

`MakeClosure` runs once per closure *creation*, which for a closure built in a
loop is once per iteration. Whatever it copies, it copies that often.

The header is per-creation because the region model says so: a heap literal is
an ordinary, reclaimable allocation born in the executing frame's region
(model.md § "Constants lower as ordinary allocations"), and a closure template
is a heap literal. The payload is not per-creation because copying a function's
whole bytecode on every iteration of a loop that builds a closure is a cost the
old representation did not have — the old blueprint was `Rc`-shared, and an
`Rc` bump is not a `memcpy`.

So the payload is materialized on first use and shared, and the header carries
a `RegionSlice<CodePayload>` of length one that names it. Building a header
copies two words and takes one cross-region reference. The rejected
alternatives:

- **Copy the payload per creation.** Every region stays self-contained, with no
  cross-region edge and no cache — and every closure creation memcpys the
  function's bytecode, constants, and location table. It trades a bounded win
  for an unbounded loss on exactly the shape (a closure in a loop) that the
  region model exists to make cheap.
- **Materialize each blueprint's header once and reference it from every
  instance.** Cheaper still — no per-creation allocation at all — but it makes
  the header's lifetime the blueprint's rather than the frame's, which is the
  promotion Rule 3 forbids, and it needs a root to pin the once-materialized
  tree. Reconsider it when the image milestone gives code objects a root that
  already outlives every instance.
- **Keep the ~20 `Rc`/`Vec` fields and clone them.** What this replaces: 13
  refcount bumps and two Rust-heap allocations (`region_table`,
  `capture_locals_mask`) per closure creation, and a `HeapObject` whose size is
  set by this one variant — 288 bytes, so a `Float` slot is ~95% padding.

## What the payload holds

Every field is inline in region pages. Nothing in a `CodePayload` owns Rust
heap memory, so the object's bytes *are* the object — the sealing property
[image.md](../image.md) § Sealing requires of body data.

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
| arity, param and local counts, signal, capture-params mask, vararg kind, WASM index | scalars, inline |

Two of those changed shape rather than merely moving.

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

## Who owns the payload region

Payloads are packed into **payload regions** the heap mints and owns, one
region serving many blueprints. The heap holds the initial reference; a header
takes an ordinary counted cross-region reference to the payload region when it
is allocated, and the free cascade releases it — the payload backing is a
`RegionSlice` in another region, so it is a recorded edge like any other
(region_slice.rs § "It is a borrowing handle").

Packing several blueprints into one region is deliberate. One region per
blueprint would mint a page per lambda — for the standard library, ~200 extra
regions and ~800 KiB of pages for ~450 KiB of payload. Blueprints materialized
close together in time are almost always the same compile unit, so packing them
together gives the compile unit one region without threading a unit identity
through the emitter. The heap opens a fresh payload region once the open one
passes a size threshold, which bounds how long a short-lived blueprint's
payload can pin a long-lived one's.

The cache maps a blueprint's address to its payload and holds a `Weak` to the
blueprint beside it. The `Weak` is not an optimization: a dead blueprint's
address can be reused by a new one, and a stale entry would hand the new
blueprint the old one's code. A lookup therefore matches the address *and*
confirms the weak still names it. Entries whose blueprint has died are swept,
and a payload region is released when the last blueprint packed into it is
gone. Teardown releases whatever is left, so a payload region is an ordinary
counted region on every path — no second reclamation mechanism, no carve-out in
the leak suite.

A header holds a strong `Rc` to its blueprint, so a blueprint cannot die while
a header made from it is alive, and the sweep cannot pull a payload out from
under a live header.

## The cache's reference is the one nothing on the heap points at

A macro expansion is a closed allocation scope: it reclaims the transformer's
scratch by balancing the references a scan of heap contents cannot explain
([rules.md](rules.md) § "Macro expansion — a closed allocation scope"). The
cache holds its reference in Rust, so that scan reaches nothing naming a
payload region — the same shape as the process roots, whose owner is the root
registry.

A payload region minted while such a scope is open is therefore excluded from
the reclaim, and a transformer that builds a closure mints one: `MakeClosure`
materializes the payload of a blueprint used for the first time. The exclusion
delays no reclamation. The region is still released when the last blueprint
packed into it dies, and by teardown otherwise.

## What the header still carries, and what removes it

The header's `Rc<TemplateProto>` is the one Rust-heap owner left on a code
object. It answers four questions the payload does not hold, and two of them
leave as their milestones land:

| Question | Answered by | Leaves with |
|----------|-------------|-------------|
| Which blueprints do my `MakeClosure` instructions index? | `child_protos` | the image milestone, when child templates become body data |
| What LIR does the JIT promote me from? | `lir_function` | the encoded-LIR side-stream ([image.md](../image.md) § JIT) |
| Where was I written? | `origin` | nothing — a `Span` is plain bytes, so the payload could hold it |
| What SPIR-V did `(git f)` compile for me? | `spirv` | nothing — the GPU path recompiles (image.md § Sealing) |

Until then the census classifies `ClosureTemplate` as sealed on the strength of
its payload, which is the part an image would carry.

## The executing context is the header

`Code` — what the dispatch loop, the tail-call trampoline, and every suspended
frame thread as the template-derived half of the execution context — is the
header plus nothing. Bytecode, constants, locations, the merge set, and the two
release tables all come from the payload; the nested-lambda blueprints and the
reserved-local count come from the blueprint. So `Code` wraps a
`ClosureTemplate` and adds no fields of its own, and swapping the executing
code object on a tail call copies two words and bumps one refcount.

The entry paths that used to build a `Code` out of a `Bytecode`'s parts build a
blueprint and materialize its payload instead, so every executing code object
in the process reaches its bytecode the same way — there is no second shape for
a synthetic thunk to drift into.
