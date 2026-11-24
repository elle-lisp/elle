# The relocation point a break opens

<!-- audited: 2026-11-24 -->

A `break` leaves its block early, so its path pays its releases at the break and past the block's exit label.

## A `break` opens a relocation point

A frame-replacing tail call is one way a path leaves before a release. A `break`
is the other. It jumps to its block's exit label, so a release the lowerer emits
while that block is still open is one the jump passed over. For a region the
break does not carry, there is no consumer to hand it to.

[The break window](anchors.md) re-anchors what it can onto the block's exit
label, and refuses on its loop barrier. A region the loop body ALLOCATES is
minted once per iteration, so one release at the exit would cover whichever
iteration's value the slot held last. That refusal is right, and it leaves one
path unserved. Every iteration that falls through runs its own release, while
the iteration that BREAKS is the last. Nothing displaces its value, and no later
release reaches it.

So the break opens a relocation point of its own, at the end of the block it
leaves. A release emitted afterwards is emitted where the solver placed it AND
replicated there. The breaking path runs the replica, and every other path runs
the placed release. The everyday shape is a drain loop whose clause body breaks,
the h2 server's DATA-frame drain ([lib/http2/server.lisp](../../../lib/http2/server.lisp)).
The function below compiles with the document and is never called, because it
needs a live stream:

```lisp
(defn drain-body [s body-parts]
  (forever
    (let [msg (s:data-queue:take)]
      (cond
        (= msg:type :data) (begin (push body-parts msg:data)
                                  (when msg:end-stream (break nil)))
        (= msg:type :error) (error msg:error)
        true (break nil)))))
```

`msg`'s last use is the second clause's TEST. So [the branch-arm
window](window.md) anchors its release at the `cond`'s own merge, and both
breaking bodies jump past it. Written with `if`, the same loop strands nothing,
because two arms leave the release inside an arm rather than at a merge the
breaks skip. That is a placement fact about the branch alone.

**The point lives exactly as long as the block is being lowered.** That makes
the count exact rather than approximate. Every position the lowerer fills while
the block is open is a position the jump passed over. The exit label is the
first position the break path reaches, and the point is gone by then. So no path
runs both copies, and the nil stamp is a second net rather than the argument.

A replica is a release firing where none fired before, so it owes what the
relocation's replicas owe ([the relocation point](replicate.md)). Escape
supplies the count argument (`frame_held_regions`), and the run must be
self-cancelling. The value the **break carries** is exempt, read off the same
operand slots a tail call's point reads. Releasing it here would free what the
block is about to hand its consumer. That value's own release stays where the
transfer pin put it, at the point the block's value is consumed.

A frame-replacing tail call in the block clears the points it dominates, the
break's among them. So a release emitted after one keeps the conservative
baseline. Dropping a license to replicate can only over-keep.

The pins:

- [region-break-loop-replica.lisp](../../../tests/impl/region-break-loop-replica.lisp),
  the reclamation, driven as rows: the `cond` clause body, the release past the
  branch's merge, the `if` and bare-break controls, and the three boundaries.
- The per-request pins in [h2-stress-scoped.lisp](../../../tests/impl/h2-stress-scoped.lisp),
  for requests that carry a body.
- The placement pins in `lir::lower::tests::release::breakexit`.
- [region-break-loop-replica-uaf.lisp](../../../tests/impl/region-break-loop-replica-uaf.lisp),
  the soundness complement. Each of these must survive the replica: a value the
  break carries out, one it carries a borrow out of, one a container outside the
  loop still holds, and one a closure captured.

## The points that cover a break

The points that cover a break cover what the break covers: the positions it
jumps over, and its block's exit label.

A `break` changes where its path pays its releases, and never what the path
owes. The path pays a release the break jumps over at the break, and a release
past the block after the exit label, where the block's other paths pay it too.
So a release that had to run ahead of a point before the break still has to
run ahead of that point after the break, in both places.

The point that matters is a tail call whose result the break carries. A `break`
out of a function's tail block walks its value as a tail value
([anchors](anchors.md)), so a closure call there replaces the frame. On that
call's closure path the frame ends at the call, before the jump. So a release
the path owes after the call runs as a replica ahead of it, or never runs.

```lisp
(defn read-all [m]
  (let [chunks @[]]
    (var k 0)
    (forever
      (when (>= k m) (break (apply concat chunks)))
      (push chunks (bytes (string/repeat "x" (inc k))))
      (assign k (inc k)))))

(assert (= (read-all 2) (bytes "xxx")) "the break carries the joined chunks")
```

`concat` is a closure, so `(apply concat chunks)` replaces the frame. The frame
releases `chunks` past the block's exit label, which the closure path never
reaches. So the release takes a replica ahead of the call, and `chunks` and
every value pushed into it are freed on each call. A native in the same place,
such as `freeze`, returns to the frame and passes the exit label.
`tls/read-all` in [lib/tls.lisp](../../../lib/tls.lisp) is the everyday shape.

The exit label is a merge: the block's fall-through and each break that targets
the block arrive there. It inherits the points that cover each targeting break,
as a branch merge inherits its arms' points ([the relocation
point](replicate.md)), and they live until the next block boundary. While the
block is open, the same points take a replica of each release the break jumps
over, beside the break's own point.

Each replica owes what any replica owes: escape's frame-held admission, a
self-cancelling run, and the exemption read per point. The tail call exempts its
operands and its own result, which is the value the break carries. On a native
fall-through the copy ahead of the call does the work, and every later copy
loads `nil`.

The pins:

- [region-break-tail-carry.lisp](../../../tests/impl/region-break-tail-carry.lisp),
  the reclamation, driven as rows: the release past the exit label, the release
  the break jumps over, and the native and `while` controls.
- The placement pins in `lir::lower::tests::release::breakexit`.
- [region-break-tail-carry-uaf.lisp](../../../tests/impl/region-break-tail-carry-uaf.lisp),
  the soundness complement. A value must survive the replica when the callee
  takes it as an argument, reaches it through a capture, or hands it back, and
  when the break carries it.
