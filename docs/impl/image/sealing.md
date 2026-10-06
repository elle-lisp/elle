# Sealing

<!-- audited: 2026-10-06 -->

What an image's body may hold, what the hydrating instance rebuilds for itself,
and what fails the dump.

[image.md](../image.md) owns the design this serves, and
[format.md](format.md) owns the streams the entries below are written into.

## What the body may hold

After the foundations, the body may contain only *sealed* heap objects:
byte-self-contained, pointing only into this image (or an image it depends
on), and free of Rust heap ownership (no `Rc`, `Vec`, `Box`, or `RefCell`
inside). Page bytes must *be* the object. Sealed objects have no real
destructors, so the hydrated region's teardown drops are no-ops by
construction.

Sealed and portable after the foundations: `Pair`, `LString`, `LArray`,
`LBytes`, `LSet`, `LStruct`, `Syntax`, closure templates, closure instances
(env is an inline `RegionSlice<Value>`), keywords and symbols (payloads are
stable name hashes), native-fns (dense `prim_id`, remapped by name), ints
and floats, `Parameter`.

A native-fn's `prim_id` is dense and process-local, so it travels as a name
([format.md](format.md) owns the stream). A def the canonical primitive
tables do not name — a trait-method handler the registry appended at run time —
fails the dump instead. Nothing loses by that refusal: those handlers are
reachable only through the default trait tables, which hydration reconstructs.

A sorted container copies in order and is never re-sorted. Every key an image
may carry ranks by its own content — a name hash for a symbol or a keyword, the
bytes for a string, its elements for an array, its structure for anything else.
So the order the dump wrote is the order the hydrating instance's comparator
agrees with, and a binary search over the mapped entries finds what it found
before. The keys that rank by address instead belong to values the dumper
refuses anyway.

## A closure crosses whole

A closure instance is three sealed fields: the template it references, its env
slice, and its squelch mask. All three cross whole, traits beside them, and
every env value goes through the ordinary walk — a capture cell in an env
snaps to its content or fails the dump (§ "Capture cells are snapped, not
persisted").

A closure template is a header naming a payload, and nothing else
([region/template.md](../region/template.md)). The payload is sealed data and
copies into the body: every slice lands in the image, constants go through the
value walk, and two headers over one payload keep one payload copy.

Everything a code object answers is therefore identical after hydration:
bytecode, constants, arity, signal, masks, locations, the region tables, the
defining span `meta/origin` reports, the child table, and the LIR the JIT
promotes from. The SPIR-V a `(git f)` compiled is not part of the code object;
it lives in a cache on the VM, and the GPU path recompiles in a fresh process
(§ "What the body refuses").

The defining span is twenty bytes of plain data. Its file id is the one
process-local number a payload holds, and it travels by name like a syntax
node's ([format.md](format.md)).

The LIR is the payload's `lir` body ([lir.md](../lir.md) § "The frozen form").
Its records hold indices and plain bits, so they copy as bytes, with every pad
written as zero. The values its `ValueConst` instructions load go through the
value walk, where the copy of the constant pool has already met each of them.
Its file table holds spellings, which copy like a string's bytes and need no
file stream. A hydrated closure therefore reaches the JIT exactly as a
source-booted one does.

A closure a compiled WASM module built fails the dump by name: its dispatch
index names a function table of the module this process holds, which no other
process can reopen.

## A child code object crosses as a header

A payload carries a **child table**: the headers this function's `MakeClosure`
instructions index, in instruction order. The emitter fills it, so every
payload carries one, live or hydrated ([region/template.md](../region/template.md)).
A `MakeClosure` reads the child's payload out of the table and builds a fresh
region-local header over it, so the header it builds is the same allocation in
the same region under both boots.

The dumper copies a child like any other header: the child's payload goes
through the payload copy, and the copied header goes into the copied parent's
table. A child's own children are its payload's child table, so a nest of any
depth crosses by one rule. A child payload two parents name copies once,
exactly as a payload two headers name does. A child that dispatches into a WASM
module fails the dump where any other WASM closure does.

## Capture cells are snapped, not persisted

The stdlib file-letrec allocates one `CaptureCell` (`Rc<RefCell<Value>>`) per
captured top-level binding. After the letrec fixpoint completes, a cell whose
binding is never `assign`ed holds its final value. The dumper snaps such a
cell: the copy references the content directly, with no cell between. The
sharing map keys the cell, so every closure that captured it hydrates naming
one copy of the content. Snapping is read-transparent by the env read's own
rule — a non-cell slot is pushed as it is, and only an `assign` needs the
cell back.

The compiler knows which top-level bindings are assigned anywhere in the
file, and the mint of a compiled forward cell records that fact on the cell,
the binding's name beside it. A cell whose binding is assigned fails the
dump, naming the binding: its content is post-boot-mutable state, which the
strict policy refuses outright and the environment policy routes through the
side-stream. The boot image therefore requires stdlib to have no assigned
top-levels — a property the dump enforces, and a reasonable one to demand of
a standard library. [measurements.md](measurements.md) item 2 counts the boot
graph's cells; stdlib assigns no top-level, so every one of them snaps.

A cell minted at run time — a captured parameter, a captured lambda-local —
records no binding and never snaps: it fails the dump as unsealed data,
named by variant.

## What the body refuses

Refused outright: every mutable variant, `LBox`, `Fiber`, thread and library
handles, ports, externals, FFI signatures, managed pointers.

A closure whose signal or squelch mask names a signal this process *declared*
is refused too, by the signal's name. Bits from 32 up are handed out in
declaration order by a process-global registry, and no image carries a signal
table, so such a bit would mean a different signal — or none — in the instance
that hydrates it. The test is against what the registry handed out rather than
against the whole high range: inference sets high bits to mean "this may signal
anything", and those mean the same on both sides of a dump. One consequence is
worth knowing: a core.lisp closure carries such an inference mask, so a process
that has declared a signal can no longer dump a boot image. A boot dump runs
before any program does, so the order holds where it matters. Mutable
*bindings* may still be persisted through the side-stream where the image's
dump policy permits it — the environment policy does, opt-in; the strict boot
policy does not ([image.md](../image.md) owns the fork). The SPIR-V kernel
cache is VM state, not a code-object field, so nothing in it crosses: the GPU
path recompiles.

## Process-owned resources reconstruct in place

The boot graph is not fully pure: stdlib defines `*stdin*`/`*stdout*`/
`*stderr*` as dynamic parameters, and a `Parameter` heap object — itself
sealed POD (`{id, default, traits}`) — holds as its *default* an `External`
wrapping the stdio port. `send` already made the semantic call for this case:
a stdio port is reconstructed fresh on the receiving side, never carried.
The image does the same via the **reconstruction stream**: (slot location,
constructor tag) entries emitted by the dumper wherever it meets a
reconstructible resource. Hydration runs each constructor, allocates the
fresh value into a companion region, and writes the pointer into the listed
slot — a handful of dirtied frames. The companion region is an ordinary
region whose edge from the hydrated region is recorded, so the teardown
cascade releases it.

Reconstruction must be in place, not re-evaluation of the defining forms:
closures like `println` capture the `Parameter` object itself, so a
re-evaluated `def` would mint a second parameter the captured references
never see. Anything the dumper meets that is neither sealed nor
reconstructible nor side-streamable fails the dump with a named binding.

## A traits slot has three answers

The **default trait tables** are the first reconstructible class, found by
the census ([measurements.md](measurements.md) item 2). Every collection the
runtime allocates carries a `traits` field pointing at one of the instance's
two default traitsets — `@struct`s built by `init_default_traits` at VM init,
before any stdlib load or hydration. They are instance infrastructure, not
program state, so the dumper never copies them. A `traits` slot aimed at a
default traitset becomes a reconstruction entry whose constructor resolves the
hydrating instance's own table for that tag. The tables exist before hydration
by construction (VM-init order), so the constructor is a lookup, not an
allocation. The dumper tests the slot against the whole default table rather
than against the object's own tag, because one traitset serves seven tags and
a program may attach the array's table to a string.

A `traits` slot naming anything else is program data and copies into the body
like any other struct. `with-traits` attaches an ordinary immutable struct
whose methods are native-fns or closures the body already carries, so a user
traitset needs no mechanism of its own. A traits slot therefore has three
answers: nil, a reconstruction entry, and an ordinary pointer relocation.

## A parameter's default has the same three

A `Parameter`'s `default` slot is the one slot a boot graph holds a
reconstructible resource in, so it is the only slot the dumper looks for one
in. A default naming an `External` over a standard stream becomes a
reconstruction entry tagged with the stream; every other default copies into
the body. An `External` the walk meets anywhere else fails the dump, because
no entry could name the slot to rebuild it into. A file or socket port fails
the dump wherever it sits: the port owns a descriptor this process opened, and
no other process can reopen it from the image. `send` refuses both cases
already, and for the same reasons.

A parameter's `id` crosses unchanged, because resolution is by id — a fiber's
parameter frames name the parameter they rebind by that number, and so do the
closures the body carries. A fresh instance mints ids from a counter of its
own, so the image records a watermark and hydration raises that counter past
it ([format.md](format.md) owns the rule). Without the raise, the first
parameter a hydrated program builds carries `*stdin*`'s id, and `parameterize`
over either one rebinds both.

## Macros persist whole

A manifest macro entry carries its parameter lists, its template syntax (a
body value), and its transformer cache's body location. The filled caches —
ordinary closures — hydrate without recompiling, preserving the hygiene
property the lazy fill exists for. The persisted transformer is the one
compiled in a real expansion context, which is exactly what later compiles
reuse in a source boot. A cache the boot never filled stays empty and fills
lazily as today.
