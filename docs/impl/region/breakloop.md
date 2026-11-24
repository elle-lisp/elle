# A value a loop makes and a break carries out

<!-- audited: 2026-11-24 -->

A region a loop makes and a `break` carries out takes two releases: one on each pass that loops on, one where the block's value dies.

## One release cannot cover every pass

A `break` hands its value to its block, so the transfer pin puts the value's
release where the block's value is consumed ([anchors](anchors.md)). That
point lies past every loop inside the block. When such a loop makes the value,
each pass that loops on makes a new one. The one release past the block then
reaches only the value of the pass that broke, and every earlier pass strands
its region.

```lisp
(defn carried [m]
  (var k 0)
  (forever
    (let [b (string/repeat "x" (inc k))]
      (when (>= k m) (break b)))
    (assign k (inc k))))

(assert (= (carried 2) "xxx") "the break carries the third pass's value")
```

`(carried 2)` makes three strings and releases one of them with one release
alone. The loop barriers of the [break window](anchors.md) and the
[branch-arm window](window.md) keep a release in the loop for a region the loop
makes. The transfer pin has no such barrier, because the carried value has to
outlive the block.

## Two releases

A region qualifies when all four of these hold:

- a `While` or `Loop` inside the block allocates it;
- a `break` inside that loop, targeting the block, carries it;
- its release takes the value route, which loads its binder's slot
  (`RegionInfo::value_routed_regions`);
- no other mechanism releases it, and this frame holds it alone. The
  exclusions are the branch window's: a merge child, a co-owned group member,
  a cell release, a mutated binder's value, a suppressed release, and a region
  the [frame-held admission](held.md) refuses.

Each pin of such a region is read by where it lands. A pin inside the loop's
body moves `decref_point`, which becomes the **pass release**. A pin outside the
loop's body moves `exit_point`, which becomes the **exit release**. The transfer
pin is one of these, and so are a binding of the block's value and a read
after the block. `lifetime_point` takes the later of the two, because the
ownership and merge cuts read the region's whole life.

The pass release is placed as any release in a loop is. The carried value's
last use inside the loop is the `break`, and a release there lands in the dead
block after the jump. The branch window moves it onto the branch whose arm
holds the `break`, the first point a pass that loops on reaches. The break
window's loop barrier keeps it inside the loop.

| Path | The pass release | The exit release |
|------|------------------|------------------|
| A pass that loops on | Releases the pass's value and stamps the slot `nil` | Not reached |
| The pass that breaks | Jumped over | Releases the carried value after its consumer |
| The loop ends with no break | Ran on the last pass | Loads `nil` and releases nothing |

Both releases take the value route, and that makes the pair count once on every
path. A release by region id leaves no stamp, so an id release at the exit
would free a region the last pass already freed. A value release loads the
slot, releases that value's region and stamps the slot `nil`. A second release
on the same path therefore loads `nil` and does nothing, the property the
replicas rest on ([replicate](replicate.md)). A region that fails any of the
four conditions keeps one release at the exit, and strands as before.

The pass release fires on passes where no release fired before, so it owes a
count argument. On a pass that loops on, nothing has carried the value out. A
container that stored it took a count of its own, and a closure that captured
it holds a counted edge. The frame-held admission refuses the other holder
within reach, an uncounted borrow in a parked frame.

## The lowerer

The lowerer emits the pass release at `decref_point` and the exit release at
`exit_point`. Each asks `release_route` as every release site does
([the region instructions](../../../src/lir/lower/regions.md)), and each takes
the value route. The exit release is exempt from the break's relocation point
for the reason the carried value is: it frees what the block hands its
consumer.

The pins:

- [region-break-loop-carry.lisp](../../../tests/impl/region-break-loop-carry.lisp),
  the reclamation, driven as rows. It holds the `forever` and `while` faces,
  a block value bound to a name, a use after the branch, and the control rows
  that already read zero.
- `region::infer::tests::blocks`, the two points and the four conditions.
- [region-break-loop-carry-uaf.lisp](../../../tests/impl/region-break-loop-carry-uaf.lisp),
  the soundness complement. The carried value must survive both releases when
  it is returned, bound and read after the block, or stored in a container
  outside the loop. A value an earlier pass stored must survive the later
  passes' releases.
