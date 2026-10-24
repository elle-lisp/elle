# The verifier

<!-- audited: 2026-10-21 -->

The two passes every hydration runs, which refuse a damaged or drifted image by
name before its region is installed.

[Hydration](hydration.md) owns where the two passes run among its steps, and
[the image file](format.md) owns the tables they read.

## Two passes, always on

The whole verifier runs in every build, in two passes: the tables before
anything is mapped, then the objects before the region is installed.

The first pass decodes every table and bounds every entry as it reads it:

- each page size is a power of two at or above the base page, the sizes
  descend, each page's cursors are in order, and the sizes sum to the section;
- each pointer relocation's slot lies inside the image and is 8-byte aligned,
  and its target lies inside it;
- each file slot is a 4-byte, 4-byte-aligned slot inside the image that names an
  entry of the file table;
- each primitive slot is an 8-byte, 8-byte-aligned slot inside the image that
  names a primitive this binary has;
- each reconstruction slot holds a whole `Value` inside the image, 8-byte
  aligned, and names a constructor this binary knows;
- each object offset admits a whole `HeapObject` and carries a tag in the
  sealed set;
- the root names an object inside the pages, or carries an immediate tag and
  no pages at all.

An immediate root is what a graph that allocated nothing produces. So pages
beside one are drift, and the root's payload word then means one thing rather
than either. The first pass reads the file's tables rather than its page bytes,
so it costs no faults. It is what stops a corrupt table from writing outside
the image during relocation.

## The object walk reads shells only

The second pass runs after relocation and reads each indexed object. Its
discriminant bytes must match the index's tag, it must lie inside its page's
object span, and every `RegionSlice` extent it names must stay inside the
inline-data span of a mapped page. It reads object shells only, never a slice's
backing bytes: an extent is checked from the `ptr` and `len` in the shell, so
the clean set is untouched. A syntax object's root node rides in its shell, so
that node's two extents are shell reads like any other. The nodes behind them
are covered by the relocation bounds check, exactly as an array's elements are.

A closure header is checked further, because what its payload names is read
back as a header rather than as data. It must name exactly one payload, at the
payload's alignment. Every slice field of the payload, and every name its three
name tables hold, must stay inside the image. Each child slot must name an
object the index itself calls a header.

The shells are already resident: relocation writes a pointer slot in every
object that has one, so the frames this walk reads are the frames it just
dirtied ([hydration](hydration.md)). An image whose objects hold no pointers at
all is the one case that pays a fault per page, and neither configuration
produces one.

## A drift detector, not a sandbox

The verifier stops format drift and damage: drift fails loudly at load rather
than as a torn read later, and a truncated or scrambled file is refused by name
instead of mapped. It does not make an untrusted image safe, because a
well-formed image is still code ([hydration](hydration.md)).
