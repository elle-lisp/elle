# Hydration

<!-- audited: 2026-10-22 -->

How an image's pages become a region of the hydrating instance: the steps, the
descriptor, and how a pointer into them resolves.

[image.md](../image.md) owns the design this serves, [the image file](format.md)
owns the sections the steps read, and [the verifier](verifier.md) owns the two
checks they run.

## The steps

1. Validate the fingerprint. On a mismatch the configuration falls back: boot
   compiles its sources, and an environment load reports the mismatch.
2. Decode and bound every table ([the verifier](verifier.md)), then replay the
   name table into the hydrating instance's display memo; one map serves both
   vocabularies. Resolve the primitive table against the live registry. Raise
   this process's parameter and static-region counters to the image's
   watermarks ([the image file](format.md)). A signal table would replay here;
   none is written yet, so a dump refuses a declared signal
   ([sealing](sealing.md)).
3. Map the page section. Reserve an aligned `PROT_NONE` range, then
   `MAP_FIXED` + `MAP_PRIVATE` each dumped page from the descriptor into its
   slot. The section's base-page file alignment makes the offsets legal, and the
   reservation gives each page the self-alignment the masked-header walk
   requires. The pages enter the region flagged **file-backed**: the pool
   neither caches nor recycles such a page, and its release is `munmap`.
   Generations, scrub's `memset`, and guardfree's `mprotect` operate on
   mappings unchanged.
4. Run the relocation passes, one linear sweep each. Rewrite each pointer slot
   to `base + offset`, write the live file id into each span that names a file,
   and write this process's `prim_id` into each primitive slot. Each write
   copy-on-write faults its 4 KiB frame private.
5. Verify the objects ([the verifier](verifier.md)), and build each
   reconstruction entry's value. An instance that cannot answer one refuses the
   load, and a refusal leaves no region behind. A constructor that allocates
   builds into one companion region.
6. Install: mint a `Counted` region over the mapped pages, stamp each page
   header, and rebuild `dtors`, `ref_objs`, the cursors and `obj_count` from the
   object index. Write each reconstructed value into its slot, and record the
   edge into the companion region, so the free cascade releases it exactly
   once.
7. Answer the root, the region at rc 1, and the scope watermark. The
   configuration does the rest: boot registers the region as a process root,
   raises its expander's scope counter, and installs the root struct's bindings
   ([booting](boot.md)).

A function's LIR needs no step: it is body data, mapped with everything else
([parity](parity.md)). A mutable binding an environment dump admits will need
none either, once its store is body data, and it grows into pages the region
claims after hydration ([mutable stores](../mutable.md)).

An environment image hydrates on top of a boot image and relocates its
cross-image slots against the boot region's hydrated pages. References between
the two hydrated regions are ordinary counted cross-region edges, recorded
during relocation; the environment region's `outgoing` table names the boot
region, nothing else.

## Each worker maps its own

A region belongs to one `RegionStore`, so each `sys/spawn` worker runs the whole
hydration against the descriptor its parent hydrated, instead of compiling the
stdlib. Each worker holds a private mapping, and the clean pages are shared
between them in the page cache ([booting](boot.md) owns what a worker
inherits). The WASM entry's host-side runtime hydrates the same way. The WASM
module itself still splices stdlib source into the compiled unit; fixing that
tier is the stdlib-module work [the WASM backend](../wasm.md) describes, not
this design.

## The input is a descriptor and an offset

**The hydrator's input is `(fd, offset)`,** never a path. A path source is
opened first, and the embedded blob maps from the executable's descriptor
([the blob](blob.md)). An image that arrives as bytes — over the network, from
Redis, from a channel — is written into an anonymous file and hydrates from
that descriptor, with no path any other process can open.

A kernel with `memfd_create` mints one directly. Every other host creates a
file under `TMPDIR` and unlinks it before writing a byte, so the descriptor is
the only way back to the bytes, and no failure downstream can leave a file
behind.

The split is by the call the platform has, not by the name it goes under:
Android is Linux to `memfd_create`, so a `target_os = "linux"` test decides the
wrong way there. Where a memfd is what was minted, it is sealed against writes,
shrinking and growing (`F_SEAL_WRITE | F_SEAL_SHRINK | F_SEAL_GROW`) before
mapping, so the immutability the mapping relies on is kernel-enforced. An
unlinked file gets no seal, and immutability is this process's discipline
instead.

**The fallback is an unlinked file rather than POSIX shared memory,** the
obvious candidate, which does not work. A Darwin shared-memory object answers
`mmap` only with `MAP_SHARED`: `pread` and `pwrite` on one fail with `ESPIPE`,
and a `MAP_PRIVATE` mapping of one fails with `EINVAL`. Private mapping is the
whole hydrator, since every page arrives through `MAP_FIXED | MAP_PRIVATE`, so a
descriptor that refuses it cannot carry an image at all. The fallback costs a
write to the filesystem where a memfd costs none, and that is the price of one
descriptor every platform treats alike.

The offset must sit on a base-page boundary of the descriptor, and a misaligned
one is refused before anything is mapped ([the image file](format.md) owns that
rule).

**Never rewrite an image file in place.** The temp-file-and-rename discipline
keeps a mapped old inode stable while a new image replaces the path;
`MAP_PRIVATE` over a file mutated in place is unspecified. Each dump writes a
temporary file no other dump names, so two dumps racing for one path, from two
processes or two threads, each rename a whole file. A sealed memfd is immune by
construction.

**An image is code — trust it like a shared library.** Hydration installs raw
memory and executes whatever the templates say. The verifier bounds-checks
pointers and tags, and it is a drift detector, not a sandbox. Hydrate only
images from sources you would `dlopen`; a network-retrieved image deserves the
policy a network-retrieved `.so` gets.

## A pointer into an image resolves by interval

Resolving a pointer to its region (`region_of_ptr`) is a probe ladder. It masks
the address to each size-class base and tests the 16-byte header, then confirms
with `RegionPool::owns`, a linear scan of the region's page list. Most regions
are one or two pages, so both steps are cheap.

A single region holding all of stdlib breaks both assumptions. A pointer into a
large page fails every smaller class's header probe first, each a likely cache
miss into arbitrary image bytes, and the owns scan walks the whole page list.
Resolution runs on the RC hot path, at every escape incref and edge record of a
stdlib value, so that is a per-operation regression.

The mapped layout dissolves it: a hydrated region is **one contiguous address
interval by construction**, the aligned reservation of step 3. The region's
pool records that interval, so `owns` answers a pointer inside it with one
range check. The store's **interval table** lists its hydrated regions, one or
two of them. Every resolution reads the table before the probe ladder:
`region_of_ptr`, and the ownership resolver behind the allocation funnel's edge
record and the free cascade's scan. Image pointers never enter the ladder.

The interval belongs to the pool that owns the pages, so it goes when they go,
and a recycled id starts without one. A region leaves the table when its pages
are released. Page headers stay, stamped for uniformity and diagnostics, and
the hydrating store's resolution does not depend on them. A store that did not
hydrate the image resolves such a pointer through the stamped header, as it
resolves any foreign page. The stamps also argue for large image pages: each
stamped header dirties its 4 KiB frame, so fewer, larger pages keep the clean
set large.

## The clean set is the currency

Relocation copy-on-write dirties every 4 KiB frame that holds a pointer slot, a
file slot, a primitive slot or a page-header stamp; those frames pay a fault
and a private copy. A frame with none — bytecode, strings, docs, LIR records —
stays clean. It faults in lazily, so a function never executed is never read
from disk. Every process mapping the image shares the clean frames, and under
memory pressure they are evictable rather than swappable.

The dumper lays each page out as the scratch region did: object shells bump up
from the header, and slice backings bump down from the page's end. So a byte
payload shares its frames with whatever shells sit near it, and a frame holding
both is dirty. Routing byte payloads to pages of their own is how the clean set
grows from here.
