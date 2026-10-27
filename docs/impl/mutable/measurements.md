# What the sorted-store benchmark measured

<!-- audited: 2026-10-26 -->

The times an `@set` and a `@struct` took beside std's B-trees, which moved both
sorted stores onto a B-tree of nodes.

[Mutable stores in region pages](../mutable.md) owns the design these numbers
informed, and [the benchmark](../../../benches/mutable.rs) produces them.

## Method

`cargo bench --bench mutable` builds the release profile. Each row inserts
10³, 10⁴ or 10⁵ integer keys `i * 7919 mod 1000003`, then deletes every key in
the same order. The store funnels run against a `BTreeSet<Value>` or a
`BTreeMap<TableKey, Value>` on the same keys. A cell is the fastest of five
runs, in seconds; from the scanned `@struct` node on, the funnels and std run
in turn. The ratio is the larger of the insert ratio and the delete ratio. The
gate reads the 10⁵ row alone, because a phase at 10³ lasts about a tenth of a
millisecond. The runs took place on an AMD Ryzen 9 7950X under rustc
1.98.1, on 2026-10-23.

## The probe

An Elle program timed an `@set` add and delete on its sorted extent against a
`@struct` put and delete on the Rust-heap `BTreeMap`, with the same keys:

| Entries | `@set` add / del | `@struct` put / del | Ratio |
|---------|------------------|---------------------|-------|
| 10³ | 0.00046 / 0.00044 | 0.00047 / 0.00046 | ~1× |
| 10⁴ | 0.0076 / 0.0076 | 0.0050 / 0.0047 | ~1.5× |
| 10⁵ | 0.39 / 0.39 | 0.069 / 0.055 | ~6–7× |

A sorted extent at 10⁵ entries runs six times slower than a B-tree, so a
`@struct` moved onto one would fail the gate. The `@set` moved to a B-tree
first.

## The sorted extent

The benchmark at the commit that added it, before the `@set` left its sorted
extent:

| Store | Keys | Insert | Delete | std insert | std delete | Ratio |
|-------|------|--------|--------|------------|------------|-------|
| `@set` | 10³ | 0.00009 | 0.00006 | 0.00005 | 0.00004 | 1.62 |
| `@struct` | 10³ | 0.00010 | 0.00005 | 0.00005 | 0.00004 | 2.11 |
| `@set` | 10⁴ | 0.00371 | 0.00369 | 0.00079 | 0.00077 | 4.80 |
| `@struct` | 10⁴ | 0.00136 | 0.00078 | 0.00071 | 0.00076 | 1.92 |
| `@set` | 10⁵ | 0.33069 | 0.33453 | 0.01044 | 0.01017 | 32.88 |
| `@struct` | 10⁵ | 0.01542 | 0.00945 | 0.00833 | 0.00921 | 1.85 |

The run exited 1. The `@struct` row is std's `BTreeMap` behind the funnels, so
its ratio is what the funnels cost.

## The B-tree

The same benchmark after the `@set` moved onto a B-tree of nodes in one extent:

| Store | Keys | Insert | Delete | std insert | std delete | Ratio |
|-------|------|--------|--------|------------|------------|-------|
| `@set` | 10³ | 0.00013 | 0.00007 | 0.00006 | 0.00004 | 2.25 |
| `@struct` | 10³ | 0.00010 | 0.00005 | 0.00006 | 0.00005 | 1.85 |
| `@set` | 10⁴ | 0.00130 | 0.00101 | 0.00087 | 0.00083 | 1.49 |
| `@struct` | 10⁴ | 0.00150 | 0.00085 | 0.00078 | 0.00082 | 1.94 |
| `@set` | 10⁵ | 0.01412 | 0.01327 | 0.01153 | 0.01106 | 1.22 |
| `@struct` | 10⁵ | 0.01700 | 0.01017 | 0.00934 | 0.00982 | 1.82 |

The run passed. At 10⁵ keys the `@set` funnels take 1.22 times std's time, less
than the `@struct` funnels over std's own tree.

## The `@struct` tree

The same benchmark after the `@struct` moved onto a B-tree in one extent. A
put descends once, whether it rebinds a key or stores a new one, and a lookup
keeps no path:

| Store | Keys | Insert | Delete | std insert | std delete | Ratio |
|-------|------|--------|--------|------------|------------|-------|
| `@set` | 10³ | 0.00012 | 0.00006 | 0.00006 | 0.00005 | 2.07 |
| `@struct` | 10³ | 0.00016 | 0.00006 | 0.00005 | 0.00004 | 3.16 |
| `@set` | 10⁴ | 0.00124 | 0.00095 | 0.00087 | 0.00083 | 1.42 |
| `@struct` | 10⁴ | 0.00133 | 0.00097 | 0.00074 | 0.00077 | 1.80 |
| `@set` | 10⁵ | 0.01403 | 0.01289 | 0.01130 | 0.01117 | 1.24 |
| `@struct` | 10⁵ | 0.01572 | 0.01484 | 0.01007 | 0.01045 | 1.56 |

The run passed. A first build probed for a rebind and then descended again to
insert, and copied a 256-byte path out of each descent. Its `@struct` insert
took 3.05 times std's time at 10⁵ keys, and the run exited 1.

## One path per write

The same benchmark after each tree write filled one path in place, with the
descent counting the full nodes. Other work shared the machine, so this build
and the one before it ran six times each, in turn. A cell is the fastest of
the six, on 2026-10-26:

| Store | Keys | Insert | Delete | std insert | std delete | Ratio |
|-------|------|--------|--------|------------|------------|-------|
| `@set` | 10³ | 0.00012 | 0.00005 | 0.00005 | 0.00004 | 2.40 |
| `@struct` | 10³ | 0.00015 | 0.00005 | 0.00003 | 0.00003 | 5.00 |
| `@set` | 10⁴ | 0.00114 | 0.00088 | 0.00080 | 0.00078 | 1.42 |
| `@struct` | 10⁴ | 0.00117 | 0.00084 | 0.00058 | 0.00065 | 2.02 |
| `@set` | 10⁵ | 0.01244 | 0.01162 | 0.01051 | 0.01039 | 1.18 |
| `@struct` | 10⁵ | 0.01408 | 0.01296 | 0.00793 | 0.00880 | 1.78 |

Every run passed. The build before it took 0.01355 / 0.01224 for the `@set`
and 0.01510 / 0.01402 for the `@struct` at 10⁵ keys, ratios of 1.29 and
1.90. A whole run of the profiling build took 4% fewer cycles. Its loads that
waited on a partial store forward (`r0224`, `LsBadStatus2.StliOther` on Zen 4)
fell from about 48.5M to 35.3M.

A `@struct` put still moves bytes 1 to 7 of each key as two overlapping 4-byte
pieces, and the next load of them waits. A borrowed key made the insert 6%
slower. A `repr(C, u8)` key and an inline `intern_into` changed nothing. A
`u64` tag slowed std's map as much as ours, because every comparison reads it.

## Where a `@struct` write's time went

A probe inside the crate timed each layer of a `@struct` write at 10⁵ keys
after the one-path build, and was not kept. Each cell is in nanoseconds per
write, the fastest of seven runs:

| Layer | Insert | Delete |
|-------|--------|--------|
| std `BTreeMap<TableKey, Value>` | 82 | 88 |
| the bare tree of `(TableKey, Value)` | 124 | 123 |
| `StructStore::put` and `remove_entry` | 125 | 124 |
| the funnels | 138 | 129 |

The bare tree held most of the gap. A probe that scanned each node instead of
halving it cut the bare tree to 102–106 and 102 ns, with the same instructions
and a fifth fewer cycles. The binary search waited on each compare before the
next load. The same scan slowed the bare `@set` tree's insert from 122 to
130 ns. Its writes ran about 2,200 instructions each against the `@struct`
tree's 1,200, and the extra compares of a scan cost more than the wait.

The funnels resolved the container's region three times per new key and twice
per delete, where the stored values were immediates and needed none. A
`BTreeMap<TableKey, Value>` took 82 ns per insert and a `BTreeMap<Value, Value>`
113–117 ns, so std's map is the cheaper control because of its key type.

## A scanned `@struct` node

The same benchmark after a `@struct` node scanned its keys, and then after the
funnels resolved the container only for an edge. Each build ran six times, in
turn with the others, at a load of about 2.7, on 2026-10-26. A cell is the
fastest of the six:

| Build | `@set` insert / delete | `@struct` insert / delete | Ratio `@set` / `@struct` |
|-------|------------------------|---------------------------|--------------------------|
| before | 0.01249 / 0.01161 | 0.01380 / 0.01269 | 1.16 / 1.69 |
| scan | 0.01229 / 0.01172 | 0.01183 / 0.01075 | 1.14 / 1.28 |
| scan and lookups | 0.01230 / 0.01155 | 0.01145 / 0.01015 | 1.15 / 1.29 |

Every run passed. The `@struct` insert fell 17% and its delete 20%. The `@set`
moved within the noise. std's map took 0.00817 / 0.00883 beside the first build
and 0.00891 / 0.00929 beside the last, with the same std code in all three. So
the ratios understate the change, and the times compare the builds. The cause
of std's shift is not known.

## Keys side by side

The same benchmark after a node kept its keys in one array and its values in
another. The build before it and this build ran six times each, in turn, at a
load of about 2.6, on 2026-10-26. Then each ran three more times pinned to one
core with `taskset`:

| Build | `@set` insert / delete | `@struct` insert / delete | Ratio `@set` / `@struct` |
|-------|------------------------|---------------------------|--------------------------|
| interleaved, pinned | 0.01232 / 0.01169 | 0.01238 / 0.01114 | 1.13–1.16 / 1.21–1.27 |
| side by side, pinned | 0.01179 / 0.01127 | 0.01219 / 0.01093 | 1.08–1.09 / 1.22–1.23 |

Every run passed. Pinned, every run of the new build beat every run of the old
one. The `@struct` funnels gained about 2% and the `@set` funnels about 5%.
Unpinned, two of the old build's six runs ran about 10% faster in both the
funnels and std's map, so a fastest-of-six cell compared the machine rather
than the builds.

A probe of the bare `@struct` tree at 10⁵ keys, which was not kept, counted
each operation, pinned, over 20 runs of each build:

| Build | Insert / delete ns | Cycles | Instructions | L1 misses |
|-------|--------------------|--------|--------------|-----------|
| interleaved | 98.7 / 100.4 | 530 | 1189 | 28.7 |
| side by side | 94.9 / 96.7 | 503 | 1221 | 28.2 |
| std `BTreeMap<TableKey, Value>` | 79.2–85.3 / 87.0–90.2 | 448–523 | 922 | 21.4–22.0 |

The tree took 5% fewer cycles, and its L1 misses did not fall, so the
interleaved entries did not cause the misses. The probe does not show what
causes them. The `r0224` count is no guide across builds here:
std's map read 5.0–7.7 per operation beside the old build and 13 beside the
new one, with the same std code.

## Leaves and the path

The same benchmark after a leaf edit left its child indices alone and the
descent wrote its path by level. The build before it and this build ran six
times each, in turn, pinned to one core, at a load of about 2.8, on
2026-10-26:

| Build | `@set` insert / delete | `@struct` insert / delete | Ratio `@set` / `@struct` |
|-------|------------------------|---------------------------|--------------------------|
| before | 0.01173 / 0.01121 | 0.01205 / 0.01085 | 1.07–1.10 / 1.18–1.27 |
| leaves and path | 0.01183 / 0.01110 | 0.01164 / 0.01031 | 1.08–1.16 / 1.12–1.22 |

Every run passed. The `@struct` insert fell 3% and its delete 5%, and the
`@set` moved within the noise. The probe of the bare trees, pinned, counted
each operation; a cycle cell is the fastest of seven runs:

| Tree | Build | Cycles | Instructions | L1 misses |
|------|-------|--------|--------------|-----------|
| `@struct` | before | 601 | 1226 | 25.4 |
| `@struct` | leaves and path | 541 | 1066 | 25.1 |
| `@set` | before | 657 | 2288 | 14.9 |
| `@set` | leaves and path | 649 | 2159 | 14.2 |
| std `BTreeMap<TableKey, Value>` | either | 506–509 | 929–931 | 20.7–21.1 |

Of the `@struct` tree's 160 fewer instructions, writing the path by level
saved 85, the descent's loop 55, and the leaf mark 21. A `@struct` write's
descent no longer stores the child index it follows and loads it back.

Cycles depend on code layout here: std's map took 437 to 487 cycles per
operation across builds with the same std code. Only the instruction counts
tell builds apart exactly.

A key that counted its compares made 22.9 per insert and 20.6 per delete in
the `@struct` tree, against std's 23.0 and 21.2. The tree's shape does not
explain its compare time. Most of the remaining gap in L1 misses lies in
`memmove`: 6.2 per operation against std's 3.0. The extent's doubling
copies account for about 0.9 of them.
