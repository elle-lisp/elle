# Dumping

<!-- audited: 2026-10-22 -->

How the dumper turns a value graph into an image: the compacting copy, the
determinism it guarantees, and the policy for mutable bindings.

[image.md](../image.md) owns the design this serves, [sealing](sealing.md) owns
the values the copy accepts, and [the image file](format.md) owns the sections
it writes.

## A compacting copy into a real region

The dumper allocates a fresh scratch region and walks the root set with a
visited map keyed on payload address, so cycles and sharing are preserved. The
walk builds sealed forms through the ordinary arena API: cells snapped, strings
and file names interned. Every internal reference is a self-edge by
construction. A value the policy refuses fails the dump with an error naming the
binding or the variant, before any byte is written.

The emit pass then walks the scratch region's live objects once. It assembles
each page's bytes into a zeroed buffer, and records an entry in the right stream
for every pointer slot, file slot, primitive slot and reconstructed field it
meets. The file is written, and the scratch region is dropped either way.

The walk down a list's `rest` spine is a loop, not a recursion. A list's length
is bounded by memory rather than by the text that built it, so a recursive spine
walk aborts the process on a stack overflow where a loop writes the image. The
spine reserves each copy before it fills it, exactly as a closure does, so a
cycle that re-enters a pair closes onto the one copy. What hangs off the spine
still recurses, and so does every other nesting.

The root set is defined once, in [image.md](../image.md): the bindings the
dependency stack does not already provide. For the boot configuration that is
the root struct over everything boot pins as process roots
([booting](boot.md)); for an environment dump it is the session's delta.

## Determinism is engineered

Dump determinism is engineered, not assumed. The walk visits roots and children
in order and never iterates a hash map, and every table is sorted before it is
written, so the same graph always yields the same layout. Offsets, the page
table, the relocation streams and the object index are byte-identical across
dumps. The page bytes are assembled canonically from a zeroed buffer, so
headers, cursor gaps, alignment slack, and relocation slots stay zero.

The dumper never copies a record wholesale. It writes an object slot, a struct
entry, a syntax node and a code payload from the discriminant byte and the
leaf-field extents the layout probes record ([the image file](format.md)). A
struct entry's key is an enum with padding of its own, and a syntax node is a
struct with a `bool` in it around an enum. An enum copy carries uninitialized
padding from its construction temporary, and the extent copy leaves it out. LIR
records copy whole, because they have no implicit padding, and their named pads
are written as zero.

So two dumps of the same graph are byte-identical whole files. The warm cache
keys on the source digest and the binary's identity, not on a hash of the file
([booting](boot.md)). Whole-file determinism buys reproducible embedded blobs
and the check that a release binary dumps its own blob again
([the blob](blob.md)). Concurrent dumpers racing through the atomic rename
produce identical files ([hydration](hydration.md)).

## Mutable bindings are the policy fork

The strict policy, boot's, fails the dump on a mutable binding and names it. The
environment policy defaults to the same refusal with a `deep-freeze`
suggestion. `&allow-mutable` opts in, and the binding's store copies into the
body like any other value, because a mutable store lives in region pages
([mutable stores](../mutable.md)). The manifest's per-binding kind field makes
the opt-in additive. Neither the opt-in nor the region-native store is written
yet ([the plan](plan.md)), and [sending a region](../send.md) holds the policy
table every configuration reads.
