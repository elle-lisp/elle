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
runs, in seconds. The ratio is the larger of the insert ratio and the delete
ratio. The gate reads the 10⁵ row alone, because a phase at 10³ lasts about a
tenth of a millisecond. The runs took place on an AMD Ryzen 9 7950X under rustc
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
