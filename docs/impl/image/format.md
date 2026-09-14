# The image file

<!-- audited: 2026-09-14 -->

The byte layout of an image, and the fingerprint that decides whether this
binary may map it.

[image.md](../image.md) owns the design this format serves. The dumper writes
these sections and the hydrator reads them; both are in `src/image`.

## Sections

One image is one file, or one blob embedded in a larger one:

| Section | Content |
|---------|---------|
| header | magic, format version, fingerprint, section offsets, page count |
| pages | the dumped region's pages, largest first: body bytes per page at base-page-aligned file offsets, so the section is mappable. Descending size order makes the packed layout self-aligning: every earlier page's size is a multiple of every later page's, so each page's offset — in the file and in the mapped interval — is a multiple of its own size, satisfying the masked-header walk with no padding |
| page table | (size, object cursor, data cursor) per page, in placement order — rebuilds each page's cursors |
| relocations | pointer stream: (slot offset, target offset); primitive stream: (slot offset, name index); file stream: (slot offset, file index); reconstruction stream: (slot offset, constructor tag). Offsets are region-relative bytes: the hydrated region is one contiguous interval, so `base + offset` names any slot or target in O(1) and the (page, offset) pair collapses |
| object index | (offset, tag) per heap object, sorted — rebuilds `dtors`/`ref_objs` and drives the verifier |
| primitive table | the names the body's native-fn slots hold, one length-prefixed string each, sorted by name — the primitive stream indexes it |
| name table | the spellings of the symbols and keywords in the body, one length-prefixed string each, sorted by name |
| file table | the source-file names the body's spans point at, one length-prefixed string each, sorted by name — the file stream indexes it |
| signal table | user-defined signal names in dump-time bit order |
| watermarks | dump-time counters, carried in the header block: parameter id and hygiene scope id today, static-region mint and next signal bit with the milestones that need them |
| manifest | bindings: name, kind (function / macro / core), value location, signal, arity, doc location; macro entries add parameter lists, template-syntax and transformer-cache locations; inline-fn syntax locations; plus root locations and dependency fingerprints |
| side-stream | typed streams, present in any image: encoded `LirFunction`s keyed by template location (the boot configuration requires this stream); `SendValue`-encoded mutable bindings (present only where the dump policy permits mutables) |

## The pages section starts at a base-page boundary

`mmap` accepts only a file offset that is a multiple of the OS page size, and
that size is a property of the running machine rather than of the format:
Linux on x86-64 reports 4 KiB, macOS on arm64 reports 16 KiB. The header block
is therefore padded with zeros up to the first base-page boundary at or after
it, and the pages section begins there. Pinning the section to a fixed 4 KiB
start instead would map on a 4 KiB machine and fail every page with `EINVAL`
on a 16 KiB one — the header block is the only part of the file read with
`read` rather than mapped, so it is the only part whose offset is free.

## An image starts at a base-page boundary of its container

An image does not have to begin at byte zero of the descriptor that carries
it: the release build embeds one in the executable, and a fleet member may
park several in one file. Every page's file offset is then the image's own
offset plus a multiple of the base page, so `mmap` accepts the sum only when
the image's offset is itself a multiple of the base page.

The hydrator therefore refuses a misaligned offset before it maps anything,
naming the offset and the page size. The refusal is a build error rather than
a corrupt file: whoever placed the image chose the offset, and the same
descriptor is legal once the image moves to a boundary. An embedded blob meets
the rule by construction — [image.md](../image.md) § "Build integration" pins
its alignment to the largest base page any supported host reports.

## Relocation slots

A pointer slot is any 8-byte field holding an absolute address: a heap-tagged
`Value`'s payload, a `RegionSlice`'s ptr. A primitive slot is the payload word
of a `Value` with `TAG_NATIVE_FN`, which the primitive stream names.
Symbol and keyword payloads are stable hashes and need no
relocation. The dumper emits each entry as it copies the object — it knows
every variant's layout, so there is no post-hoc discovery, and targets are
region-relative offsets so hydration rewrites each slot in O(1) with no
address search.

**Static region slots** baked into bytecode operands are opaque per-function
keys; they collide harmlessly across functions. The loader bumps the global
mint counter past the image watermark anyway, so uniqueness diagnostics stay
truthful. Parameter ids, hygiene scopes, and signal bits get the same
watermark treatment.

## The name table carries spellings, not hashes

A symbol's payload is the hash of its name, so identity travels in the body and
needs no table at all. Display does not travel: a hydrating instance holds none
of the dump's spellings, and a value whose spelling it never met prints as
`#<symbol:hash>`. The name table is what closes that gap, for both vocabularies
at once ([symbol.md](../symbol.md) owns the memo it replays into).

An entry is a spelling: a byte length, the bytes, and zero padding out to the
next multiple of eight. No hash appears. The hydrator hashes each spelling with
the function the payloads already carry, so a name and the id it names cannot
disagree. Nothing derives a pointer from this section either, so its own bounds
are the whole check it needs. The entries are sorted by name, so one graph and
one set of spellings always produce one table.

The replay is a learning site, and the collision guard runs there: a spelling
whose hash the instance already maps to a different spelling panics. Checking
at the point a name lands is what extends the guard across builds, since the
two spellings meet in the receiving memo and nowhere else.

A spelling the dumping instance never learned is simply absent. The value still
hydrates, still compares equal, and still has no name to print — the memo's
standing contract, not an image rule.

## A span names its file by name, not by id

A syntax node carries a `Span`, and so does a code payload — its `origin`, the
lambda's defining span ([region/template.md](../region/template.md)). A span
carries a `FileId`: a dense index into a process-wide interner
([syntax.md](../syntax.md) owns the model). The index is an accident of which
files this process read and in what order, so it means nothing in another
process. It is the one process-local id the foundations left in the body.

So a file travels the way a primitive does: by name. The file table holds the
spellings the body's spans point at, sorted, and the file stream names each
span's `file` field with the index of its spelling. Hydration interns each
name in its own process and writes the live id into every listed slot. A slot
is four bytes, not eight, because a `FileId` is a `u32`; the verifier bounds
it and checks its alignment like any other slot.

The cost is frames. Every node of a tree read from a real file carries a file
id, so the file stream dirties every frame those nodes sit in — where an atom
node with no pointers would otherwise have stayed clean. A payload pays one
slot for its whole code object, because one origin is all it holds. Leaving
the ids alone would be wrong rather than cheap: the span would name whatever
file the hydrating process happens to have interned at that index, and print
it in an error message.

## A primitive travels by name, for the same reason

A native-fn is the immediate `Value{TAG_NATIVE_FN, prim_id}`, and that id is a
dense index into the process's primitive registry. The canonical tables seed it
in source order, and every other def — a trait-method handler, an FFI callback
— is appended in the order the process first asks for it. So the id is worth
even less across processes than a file id is, and it travels the same way.

The primitive table holds the spellings the body's native-fn slots name,
sorted; the primitive stream names each slot with the index of its spelling.
Hydration resolves each name against its own registry and writes the live id
into the payload word. The dumper zeroes that word first, so the artifact never
records which ids the dumping process happened to hand out, and two builds that
number their primitives differently write one file.

A name the canonical tables do not carry fails the dump, naming the primitive
([sealing.md](sealing.md) argues why nothing is lost).

## A reconstruction entry rewrites a whole `Value`

An entry is a slot offset and a constructor tag. A pointer relocation's target
lies inside the image, so it rewrites one payload word and leaves the tag the
canonical bytes already carry. A reconstructed value comes from the hydrating
instance instead, and neither of its words is known at dump time — so the entry
rewrites both, and the dumper leaves the slot zero.

A tag is a kind in its high word and that kind's argument in the low one, so a
constructor added later needs no re-encoding. Two kinds ship. The default
traitset names the heap tag whose table to look up, and an instance whose trait
tables are not built refuses the load by name — rather than writing nil into a
traits slot and leaving a value that answers no protocol. The standard-stream
port names which stream, and the hydrating instance opens a fresh one. That
second constructor allocates, so its value lands in a companion region and the
edge into that region is counted like any other.

## The watermarks bound what a fresh instance may mint

Two counters reach the body, and both are process-local. An expander mints
hygiene scope ids from one; a process-wide counter mints parameter ids from
zero. Each is compared for equality and nothing else, so an instance minting
from its own start hands out ids the body already uses. Two unrelated scopes
would then compare equal, and a new parameter would share `*stdin*`'s id.

The header therefore records one watermark per counter: one past the highest
value the body carries. The scope watermark masks the intro bit off first,
since intro scopes and ordinary ones come from the same counter. Hydration
raises the process's parameter counter past its own watermark, and answers with
the scope watermark for the loader that owns an expander. A body carrying
neither records zero for both.

## Fingerprint: regenerate, never migrate

`HeapObject` is `repr(Rust)`; opcode discriminants and `prim_id`s are
source-order-dependent. An image is therefore valid only for a binary whose
layout agrees with the dumper's. The fingerprint records: format version,
rustc version and target triple, `size_of`/`align_of` for `Value`,
`HeapObject`, `RegionSlice`, `Closure`, `ClosureTemplate`, and `TableKey`, the
instruction-set high-water mark, `CURRENT_EPOCH`, the feature set, a hash of
the primitive name list in registration order, the OS base page size, and
(for the boot configuration) the hashes of the three sources.

The base page size earns its place because the file's geometry is built from
it: the pages section starts at a base-page boundary and every page size in
the page table is a multiple of it. Two machines can agree on the target
triple and still disagree here — arm64 Linux ships both 4 KiB and 64 KiB
kernels. Recording the size turns that case into a fingerprint mismatch, which
falls back to sources, instead of a page table the reader rejects as corrupt
or an offset `mmap` refuses.

Size and align alone do not pin field offsets. The fingerprint therefore also
records the probed layout of every variant the dumper can emit — the heap
objects, and the `TableKey` variants a struct entry carries: the
discriminant byte and each leaf field's offset and length
([measurements.md](measurements.md) item 6 records the probe mechanism and the
measured layout). A build whose layout reorders a field or moves the
discriminant fails the fingerprint instead of hydrating garbage, so the
two-stage embed build cannot pass with a shifted layout. The same extents
drive slot canonicalization ([image.md](../image.md) § Dumping).

On mismatch the loader falls back — the `include_str!` sources never go away,
so the image is an optimization, not a correctness dependency. Images are
regenerated, not migrated; epochs stay a source-level concept. An environment
image is binary-locked: durable cross-version data belongs in files or RDF,
and the manifest is designed so a tool running the *old* binary can export
bindings as source. State that limit to users rather than promising migration.
