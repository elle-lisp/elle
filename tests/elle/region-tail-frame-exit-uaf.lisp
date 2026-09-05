(elle/epoch 12)
# Soundness complement of region-tail-frame-exit.lisp: hoisting a release from
# the dead post-`TailCall` block to just before the `TailCall` must not free
# anything the callee can still reach.
#
# The hoist moves a release EARLIER — the one place in the region system it does
# (docs/impl/region/mechanism.md § "A release past a frame-replacing tail call is
# not a release"). On the closure path it fires where none fired before, so it
# needs BOTH gates, and each witness below drives one of them. The **exemption**
# covers what the call names: an argument's release must not fire before the
# callee it was moved to reads it, and the callee closure's own region must not
# be dropped before the frame it replaces runs. The **admission** — escape
# proving the frame holds the region alone — covers the path the exemption
# cannot see: a tail callee also reaches its CAPTURED environment, which no
# argument names — but the env's hold is the funnel's counted edge, so that path
# is admitted rather than refused, and (e3) is the row that proves the count
# stands. A value the callee hands BACK rides that same edge one step further —
# the callee's `Return` mints the caller's reference after the relocated release
# has run, and the env edge is what holds the region off zero in between, which
# (e2), (e5) and (e6) drive. (e15), (e16) and (e16b) drive the other end of the
# same enumeration: a point neither route reaches cannot mint against the region
# at all, so the relocated release is the last one.
# (e7) and (e8) drive the same admission for an ENV
# CELL's `DecrefCellRegion`, whose holder is REASSIGNED: that refusal is about a
# release routed through the holder's slot, and this one names the cell box no
# `assign` repoints. (e10) and (e11) drive that same box release where it is the
# SIBLING arm's head compensation instead of the relocated copy — the arm names the
# cell's binding nowhere, so the head route covers it, and what must survive is the
# capturer's counted edge and the cell's own content. (e12), (e13) and (e14) drive
# the sibling arm that READS the cell's binding, where the box release is the tail
# compensation instead: it must post-date the arm's read, leave the capturer's
# counted edge standing, and post-date the READER of an uncounted opcode-read
# borrow out of the cell. A closure the frame RETURNS carries its capture on the
# same counted edge, so it is admitted too and the edge is what must stand (e4, and
# e9 for the cell). What the admission still refuses is a holder escape marks by a
# facet no counted edge covers: a store into a longer-lived container (f), and a
# fiber crossing (h).
#
# Every witness reads the subject's HEAP contents on the far side of the tail
# call, through a chain long enough that an over-early free faults rather than
# reading stale but still-mapped bytes. A fresh subject per iteration keeps
# region ids churning so a freed region is recycled under the reader.

# ── witnesses: a value the tail callee reaches survives the hoisted release ────

# (a) the argument is MOVED into the frame-replacing tail call and read there.
# Its release must stay in the dead block: hoisting it drops the very reference
# the callee's owned-param release consumes.
(defn a-callee (v)
  (length (first v)))
(defn a-moved (i)
  (let [x (list (string "a" i) i)]
    (a-callee x)))

# (b) the moved argument sits BESIDE a genuinely stranded one, so the hoist runs
# on this call and must skip exactly one of the two.
(defn b-moved-beside (i)
  (let [x (list (string "b" i) i)
        y (list (string "z" i) i)]
    (a-callee x)))

# (c) the CALLEE is a per-call local closure. The new activation takes over its
# release; the frame must not also drop it, or the closure is freed under the
# frame replacement that installs it.
(defn c-callee-local (i)
  (let [x (list (string "c" i) i)
        g (fn (v) (length (first v)))]
    (g x)))

# (d) the tail callee reaches the value through its CAPTURED environment — the
# shape the close exists for. The capture's incref is what the hoisted release
# must be balanced against, so the walker's read must still see live pages.
(defn d-captured (i)
  (let [src (list (string "d" i) i)]
    (letrec [go (fn (n) (if (%lt n 1) (go (%add n 1)) (length (first src))))]
      (go 0))))

# (e) the same capture, with a MUTABLE accumulator the walker writes into: the
# hoisted release drops the frame's reference to `acc` while the walker still
# pushes to it, and the caller reads the result afterwards.
(defn e-fill (dst src)
  (let [n (length src)]
    (letrec [go (fn (k)
                  (if (%lt k n)
                    (begin
                      (push dst (get src k))
                      (go (%add k 1)))
                    dst))]
      (go 0))))
(defn e-walker (i)
  (let [acc (@array)]
    (e-fill acc (list (string "e" i) i))
    (length (first acc))))

# (e2) the same walker, but the accumulator is RETURNED back out through the
# captured tail callee — the stdlib `push-all` shape, and the one that shows a
# capture is a reachability path the call's ARGUMENTS do not describe. `go` names
# `dst` only through its environment, so an exemption read off the argument list
# alone releases it before the callee that hands it back has run.
(defn e2-fill (dst src)
  (let [n (length src)]
    (letrec [go (fn (k)
                  (if (%lt k n)
                    (begin
                      (push dst (get src k))
                      (go (%add k 1)))
                    dst))]
      (go 0))))
(defn e2-walker (i)
  (let [acc (@array)]
    (length (first (e2-fill acc (list (string "x" i) i))))))

# (e3) the walker fills its captured accumulator in place and returns something
# ELSE, so nothing holds `dst` back from the relocated release and it does fire
# ahead of the tail call. What must stand is the count the funnel took when `go`'s
# environment was built: the caller reads `acc`'s contents after the call, and a
# release that reached zero would have recycled those pages.
(defn e3-fill (dst src)
  (let [n (length src)]
    (letrec [go (fn (k)
                  (if (%lt k n)
                    (begin
                      (push dst (get src k))
                      (go (%add k 1)))
                    n))]
      (go 0))))
(defn e3-walker (i)
  (let [acc (@array)]
    (e3-fill acc (list (string "y" i) i))
    (length (first acc))))

# (e5) the same hand-back, with this frame holding the ONLY other reference: the
# accumulator is minted at the call site and MOVED into the walker by a tail call,
# so the caller kept nothing. The counted edge `go`'s environment took is then the
# single thing between the relocated release and the `Return` that mints the
# caller's reference — a release that reached zero recycles the very pages read
# here.
(defn e5-drive (i)
  (e2-fill (@array) (list (string "m" i) i)))
(defn e5-read (i)
  (length (first (e5-drive i))))

# (e6) the hand-back reached through a branch ARM, so the release is the merge's
# replica ahead of the arm's `TailCall` rather than an in-block move, and the
# funding edge is read at that arm's own relocation point.
(defn e6-arm (v t)
  (let [g (fn () v)]
    (if t (g) 0)))
(defn e6-read (i)
  (length (first (e6-arm (list (string "o" i) i) true))))

# (e7) the relocated release is an ENV CELL's `DecrefCellRegion`, and the tail
# callee is the very closure that captured the cell — it rewrites the cell and
# reads it back. The release names the cell BOX, whose only other reference is the
# counted edge the funnel took when `g`'s environment was built; if it reached zero
# the callee's `StoreCapture` would rebind a freed cell and the caller's read would
# tear. The holder is REASSIGNED, which is exactly the refusal that must be scoped
# to slot-routed releases and not to this one.
(defn e7-cell (i)
  (def @c (list (string "q" i) i))
  (let [g (fn ()
            (assign c (first c))
            (length c))]
    (g)))
(defn e7-read (i)
  (e7-cell i))

# (e8) the same cell reached through a branch ARM, so the release is the merge's
# replica ahead of the arm's `TailCall`.
(defn e8-arm (v t)
  (def @c v)
  (let [g (fn ()
            (assign c (first c))
            (length c))]
    (if t (g) 0)))
(defn e8-read (i)
  (e8-arm (list (string "s" i) i) true))

# (e9) the closure holding the reassigned cell is RETURNED, carrying the box on the
# counted `closure ⊇ cell` edge: the returned closure rewrites and reads its cell
# after the frame is gone, so that edge is what the sibling arm's release must leave
# standing.
(defn tail-zero ()
  0)
(defn e9-escaping (v t)
  (def @c v)
  (let [g (fn ()
            (assign c (first c))
            (length c))]
    (if t g (tail-zero))))
(defn e9-read (i)
  (let [g (e9-escaping (list (string "t" i) i) true)]
    (g)))

# (e10) the FALLING-THROUGH arm of the same branch, where the box's release is the
# head compensation rather than the relocated copy — and the closure holding the
# cell leaves through that very arm. The head release drops the frame's own env-slot
# reference; what must stand is the counted `closure ⊇ cell` edge the funnel took
# when `g`'s environment was built, since the caller rewrites and reads the cell
# through `g` after the frame is gone.
(defn e10-arm-escape (v t)
  (def @c v)
  (let [g (fn ()
            (assign c (first c))
            (length c))]
    (if t (g) g)))
(defn e10-read (i)
  (let [g (e10-arm-escape (list (string "u" i) i) false)]
    (g)))

# (e11) the same falling-through arm where nothing carries the cell out: the box
# dies on this path, and its CONTENT — a value the caller still owns — must
# outlive the head release, which names the box and never unwraps to the content.
(defn e11-arm-drop (v t)
  (def @c v)
  (let [g (fn () (length c))]
    (if t (g) 0)))
(defn e11-read (i)
  (let [v (list (string "v" i) i)]
    (e11-arm-drop v false)
    (length (first v))))

# (e12) the sibling arm READS the cell's binding, so its box release is the TAIL
# compensation — after that read rather than at the arm's head — and the arm hands
# the cell's CONTENT back. The release names the box, and the box's cascade drops
# the one reference the cell holds on that content; the caller reads the returned
# value afterwards, so a cascade that reached it would tear.
(defn e12-arm-read (v t)
  (def @c v)
  (let [g (fn () (length c))]
    (if t c (g))))
(defn e12-read (i)
  (length (first (e12-arm-read (list (string "w" i) i) true))))

# (e13) the same reading arm where the capturer has already ESCAPED into a
# module-level slot. The tail release drops the frame's own env-slot reference and
# nothing else — what must stand is the counted `closure ⊇ cell` edge the funnel
# took, since the caller drives the escaped closure after the frame is gone and
# every drive derefs the box.
(var e13-kept nil)
(defn e13-arm-read-escape (v t)
  (def @c v)
  (let [g (fn () (length (first c)))]
    (assign e13-kept g)
    (if t (length c) (g))))
(defn e13-read (i)
  (let [n (e13-arm-read-escape (list (string "x" i) i) true)
        m (e13-kept)]
    (+ n m)))

# (e14) the reading arm's use is an uncounted OPCODE read, whose result is a
# borrow living inside the cell's content. The box's release must post-date the
# READER, not the read, or the cascade frees the page the borrow points into.
(defn e14-arm-borrow (v t)
  (def @c v)
  (let [g (fn () (length c))]
    (if t (length (first c)) (g))))
(defn e14-read (i)
  (e14-arm-borrow (list (string "y" i) i) true))

# (e15) a returned hand-back at a point the tail callee cannot reach. `v` reaches a
# return through the OTHER arm, so the arm that leaves through the callee releases a
# return-frontier region ahead of that call — and the callee neither names nor
# captures it, which is why nothing there can mint against it. Both arms are driven:
# the returning one runs its mint before the anchored release, so the caller's read
# must still see live pages, and the leaving one must free without touching the
# caller's own copy.
(defn e15-sink ()
  0)
(defn e15-arm (v t)
  (if t v (e15-sink)))
(defn e15-read (i)
  (let [w (list (string "aa" i) i)
        n (e15-arm (list (string "ab" i) i) false)
        m (length (first (e15-arm w true)))]
    (+ n (%add m (length (first w))))))

# (e16) the everyday shape of the same reading — an index-walk fold driver. Each
# step hands the COMBINER the previous accumulator and the tail callee the
# combiner's RESULT, so the displaced accumulator's release is the frame's own and
# runs ahead of the recursive call. The combiner reads what it was handed on the
# next step, and the caller reads what the last step returned.
(defn e16-step (f n j acc)
  (if (%lt j n) (e16-step f n (%add j 1) (f acc j)) acc))
(defn e16-read (i)
  (let [seed (list (string "ac" i) i)
        out (e16-step (fn (a b) (list (string "ad" i) (length (first a)))) 3 0
                      seed)]
    (length (first out))))

# (e16b) the same driver whose combiner hands its accumulator straight back, so the
# region the frame releases is one the tail callee does receive — through the
# combiner's own return mint, which is what must stand.
(defn e16b-read (i)
  (let [seed (list (string "ae" i) i)
        out (e16-step (fn (a b) a) 3 0 seed)]
    (length (first out))))

# (e17) a `letrec` closure whose body's tail is a BRANCH. Every arm leaves through
# its own callee, so the scope-end release is replicated ahead of each arm's
# `TailCall` — and replication needs the closure's VALUE route, which loads the
# slot the `letrec` binder recorded and frees the region that value lives in
# (docs/impl/region/mechanism.md § "Self-cancelling is a property of the ROUTE, not
# of the region's class"). The closure captures a caller-owned list, so the free
# cascades along the funnel's counted edge and must drop that edge alone: the
# caller hands ONE list to both arms and reads it afterwards, so an over-cascade
# faults on the second call or on the final read. Exactly one release must run per
# path, or the second call walks a recycled page.
(defn e17-sink (n)
  n)
(defn e17-arm (v t)
  (letrec [go (fn (m) (if (%lt m 1) (length (first v)) (go (%sub m 1))))]
    (let [n (go 3)]
      (if t (e17-sink n) (e15-sink)))))
(defn e17-read (i)
  (let [v (list (string "af" i) i)
        n (e17-arm v true)
        m (e17-arm v false)]
    (%add n (%add m (length (first v))))))

# (e18) the merge a branch inherits from its ENTRY rather than from its arms
# (docs/impl/region/mechanism.md § "A merge inherits what covered the branch's
# ENTRY as well"). Functionalization inserts a second branch to merge the two
# versions of the mutable this arm reassigns, so the scrutinee's release is
# emitted past it and replicated ahead of the arm's `TailCall`. The scrutinee is a
# pair built around a list the CALLER owns, so freeing it cascades one decref
# along the funnel's counted edge — which must drop that edge and no more. The
# caller drives both arms with one list and reads it afterwards, so an
# over-cascade faults on the second call or on the final read.
(defn e18-pair (v)
  [v 1])
(defn e18-arm (v t)
  (let [[a b] (e18-pair v)
        g (fn () (length (first a)))]
    (if t
      (let [_ (begin
                (def @i 0)
                (assign i (%add i 1))
                i)]
        (g))
      5)))
(defn e18-read (i)
  (let [v (list (string "ag" i) i)
        n (e18-arm v true)
        m (e18-arm v false)]
    (%add n (%add m (length (first v))))))

# (e4) the capturing closure is RETURNED, so it outlives the frame and carries `x`
# with it — on the funnel's counted edge, which is what the frame's release must
# leave standing. The caller invokes the closure afterwards and reads through it.
(defn e4-escaping (i)
  (let [x (list (string "n" i) i)]
    (fn () (length (first x)))))
(defn e4-read (i)
  ((e4-escaping i)))

# (f) the stranded value ESCAPES into a container that outlives the frame before
# the tail call. The store's incref is what the hoisted release must leave
# standing — dropping the producer's reference is correct, freeing the stored
# value is not.
(def @sink @[])
(defn tail-sink ()
  0)
(defn f-escaped (i)
  (let [x (string "f" i)]
    (push sink x)
    (tail-sink)))
(defn f-read (i)
  (f-escaped i)
  (length (get sink (%sub (length sink) 1))))

# (g) the stranded value is RETURNED by the enclosing function through the tail
# callee, so the caller's read must see it alive.
(defn g-hand-back (v)
  v)
(defn g-returner (i)
  (let [x (list (string "g" i) i)]
    (g-hand-back x)))
(defn g-return (i)
  (length (first (g-returner i))))

# (h) the tail callee YIELDS across a fiber boundary while holding the argument:
# the parked frame resolves the value after the hoisted release would have run.
(defn h-yielder (v)
  (emit :yield (length (first v))))
(defn h-fiber (i)
  (let [fb (fiber/new (fn () (h-yielder (list (string "h" i) i))) |:yield|)]
    (fiber/resume fb)))

# ── the per-arm face: a branch merge inherits its arms' relocation points ─────
# A release emitted past the merge is replicated ahead of each arm's `TailCall`
# and still emitted at the merge, so both obligations are re-asked per point and
# a third appears: the two copies must act exactly ONCE on a path that reaches
# both.

# (i) the argument is moved into ONE arm's tail call and read there. The
# exemption is per point, so that arm must keep its release in the dead block
# even though its sibling is free to take a copy.
(defn i-arm-moved (v t)
  (if t (a-callee v) 0))
(defn i-arm (i)
  (i-arm-moved (list (string "i" i) i) true))

# (j) the arm's tail callee reaches the value through its CAPTURED environment —
# the admission, now asked of a replica rather than of an in-block move.
(defn j-arm-captured (v t)
  (let [g (fn () (length (first v)))]
    (if t (g) 0)))
(defn j-arm (i)
  (j-arm-captured (list (string "j" i) i) true))

# (k) the arm's tail call is to a NATIVE, which pushes no frame and falls
# through to the merge — so this path reaches the replica AND the merge copy.
# Acting twice drops the reference the CALLER still holds, freeing `x` under its
# read. This is the row the self-cancelling requirement exists for.
(defn k-arm-native (v w t)
  (if t (length w) 0))
(defn k-arm (i)
  (let [x (list (string "k" i) i)]
    (k-arm-native x (string "w" i) true)
    (length (first x))))

# ── the forward-cell face: a region no binding names ──────────────────────────
# A prebound FORWARD CELL is reached through its binding's verdict rather than its
# own, because a binding names the closure region its cell points AT (see
# docs/impl/region/mechanism.md § "A compiled capture cell is frame-held exactly as
# its binding is"). The relocated release drops the frame's slot reference; what
# must stand is the counted `closure ⊇ cell` edge the capturer's env took, since
# every later `DerefCell` that dispatches the sibling resolves through the cell.

# (l) the sibling is reached through the cell on every step of the recursion, and
# reads a heap value of its own. The cell's release now fires ahead of the letrec
# body's tail call, so a release that reached zero recycles the cell under the very
# read that dispatches `helper`.
(defn l-fwd (i)
  (let [src (list (string "l" i) i)]
    (letrec [helper (fn (k) (%add k (length (first src))))
             go (fn (k) (helper k))]
      (go 0))))
(defn l-read (i)
  (l-fwd i))

# (m) the CAPTURER is handed back, so what stands between the relocated release and
# the caller's minted reference is the callee's `closure ⊇ cell` edge. The caller
# drives the returned closure afterwards, and every step of that drive derefs the
# cell and reads `src` through the sibling, all after the defining frame is gone.
(defn m2-fwd (i)
  (let [src (list (string "m" i) i)]
    (letrec [helper (fn (k)
                      (when (%not (%int? k)) (error :k))
                      (%add k (length (first src))))
             go (fn (k)
                  (when (%not (%int? k)) (error :k))
                  (if (%lt k 1)
                    go
                    (begin
                      (helper k)
                      (go (%sub k 1)))))]
      (go 3))))
(defn m2-read (i)
  (let [g (m2-fwd i)]
    (if (nil? (g 3)) 0 1)))

# (n) the cell's own CONTENT is handed back: `go` returns `helper`, so the sibling
# outlives the cell that held it and the caller calls it directly. The cell's
# relocated release must leave the sibling's region standing on the caller's mint.
(defn n2-fwd (i)
  (let [src (list (string "n" i) i)]
    (letrec [helper (fn (k)
                      (when (%not (%int? k)) (error :k))
                      (%add k (length (first src))))
             go (fn (k)
                  (when (%not (%int? k)) (error :k))
                  (if (%lt k 1) helper (go (%sub k 1))))]
      (go 2))))
(defn n2-read (i)
  ((n2-fwd i) 0))

# (s) the SIBLING captures the self-recursive member and the letrec body tail-calls
# the sibling, so ONE tail call carries both deferred channels — the merged arena's
# `deferred_release_slot` and the sibling's own `defer_callee_release`
# (docs/impl/region/letrec.md § "The arena channel and the callee channel are
# independent"). Each drops a different frame reference; running either twice takes
# a live region to zero and recycles its pages under the next call's walk. `go`
# reads a heap value of its own on every step, and reaches it through the cell the
# arena holds.
(defn s-sib (i)
  (let [src (list (string "s" i) i)]
    (letrec [go (fn (k)
                  (when (%not (%int? k)) (error :k))
                  (if (%lt k 1) (length (first src)) (go (%sub k 1))))
             outer (fn (k)
                     (when (%not (%int? k)) (error :k))
                     (go k))]
      (outer 3))))
(defn s-read (i)
  (s-sib i))

# (t) the letrec body tail-calls the CAPTURED sibling rather than the capturer, so
# the callee's own region is the one the relocation must leave alone and the
# deferral is what runs its release — at the callee's normal completion, never
# before the frame it replaces has run (docs/impl/region/mechanism.md § "What the
# exemption keeps, a channel must still run"). `helper` reads a heap value through
# its OWN environment on the far side of the frame replacement, so a release that
# fired ahead of the `TailCall` recycles the pages that read resolves through, and
# one that ran twice recycles them under the caller.
(defn t-member (i)
  (let [src (list (string "t" i) i)]
    (letrec [helper (fn (k)
                      (when (%not (%int? k)) (error :k))
                      (%add k (length (first src))))
             go (fn (k) (helper k))]
      (helper (go 0)))))
(defn t-read (i)
  (t-member i))

# ── the operand-value face: what an argument's evaluation merely USED ─────────
# The exemption reads an operand's VALUE, not its syntax (docs/impl/region/mechanism.md
# § "What an operand names is its VALUE, not its syntax"), so a region reached only
# inside an argument's own nested call is no longer exempt and its release does fire
# ahead of the `TailCall`. Three ways the value can still live in such a region, each
# with a different reference standing between the relocated release and the callee's
# read.

# (o) the argument's nested call HANDS BACK one of its own arguments, so the value
# moved into the tail call lives in the region the release just dropped. The
# callee's `Return` minted for it, and that mint is what must stand.
(defn o-ident (v)
  v)
(defn o-passthrough (v)
  (a-callee (o-ident v)))
(defn o-read (i)
  (o-passthrough (list (string "o1" i) i)))

# (p) the argument's nested call is a native container READ, whose result is a
# borrow living INSIDE the container. The pass-through retain the native took is
# the reference the relocated release must leave standing.
(defn p-callee (s)
  (%add 1 (length s)))
(defn p-read-elem (v)
  (p-callee (first v)))
(defn p-read (i)
  (p-read-elem (list (string "p1" i) i)))

# (q) the argument is an inline `%`-opcode read — which mints no region, so its
# result lives in the operand's region with no reference of its own. That is why the
# container's own release is extended to the reader (Rule 4) and lands in the dead
# block: the operand is the value-producing leaf and stays exempt, because hoisting
# the container's release would free the page the callee is handed.
(defn q-read (i)
  (let [v (%pair (string "q1" i) nil)]
    (p-callee (%first v))))

# (r) the argument is a fresh LAMBDA capturing a local, so the closure region is
# the moved value and stays exempt while the capture's own counted edge is what
# keeps the captured value alive under the callee's call.
(defn r-call-thunk (g)
  (length (first (g))))
(defn r-lambda-arg (i)
  (let [x (list (string "r1" i) i)]
    (r-call-thunk (fn () x))))

# ── controls: the same reads with a NATIVE tail call — correct now ────────────
(defn c-plain (i)
  (let [x (list (string "p" i) i)]
    (length (first x))))

# ── drive: fresh subject each iteration; an over-early free faults on the read ─
# The iteration count is a PAGE budget, not a confidence knob: `--trace=guardfree`
# leaks every freed page `mprotect(PROT_NONE)`'d, so witnesses × iterations is
# bounded by the process's map count and an added witness has to be paid for out of
# the loop bound. A freed region is recycled within a handful of iterations, which is
# what the pin actually rests on — so the bound is set for headroom, not for reach.
(var i 0)
(var a 0)
(var b 0)
(var c 0)
(var d 0)
(var e 0)
(var e2 0)
(var e3 0)
(var e5 0)
(var e6 0)
(var e7 0)
(var e8 0)
(var e9 0)
(var e10 0)
(var e11 0)
(var e12 0)
(var e13 0)
(var e14 0)
(var e15 0)
(var e16 0)
(var e16b 0)
(var e17 0)
(var e18 0)
(var e4 0)
(var f 0)
(var g 0)
(var h 0)
(var ai 0)
(var aj 0)
(var ak 0)
(var k 0)
(var l 0)
(var m2 0)
(var n2 0)
(var s1 0)
(var t1 0)
(var o1 0)
(var p1 0)
(var q1 0)
(var r1 0)
(while (%lt i 1350)
  (assign a (a-moved i))
  (assign b (b-moved-beside i))
  (assign c (c-callee-local i))
  (assign d (d-captured i))
  (assign e (e-walker i))
  (assign e2 (e2-walker i))
  (assign e3 (e3-walker i))
  (assign e5 (e5-read i))
  (assign e6 (e6-read i))
  (assign e7 (e7-read i))
  (assign e8 (e8-read i))
  (assign e9 (e9-read i))
  (assign e10 (e10-read i))
  (assign e11 (e11-read i))
  (assign e12 (e12-read i))
  (assign e13 (e13-read i))
  (assign e14 (e14-read i))
  (assign e15 (e15-read i))
  (assign e16 (e16-read i))
  (assign e16b (e16b-read i))
  (assign e17 (e17-read i))
  (assign e18 (e18-read i))
  (assign e4 (e4-read i))
  (assign f (f-read i))
  (assign g (g-return i))
  (assign h (h-fiber i))
  (assign ai (i-arm i))
  (assign aj (j-arm i))
  (assign ak (k-arm i))
  (assign l (l-read i))
  (assign m2 (m2-read i))
  (assign n2 (n2-read i))
  (assign s1 (s-read i))
  (assign t1 (t-read i))
  (assign o1 (o-read i))
  (assign p1 (p-read i))
  (assign q1 (q-read i))
  (assign r1 (r-lambda-arg i))
  (assign k (c-plain i))
  # The sink is a module-level container by design (witness f stores into it);
  # drain it so the driver's own retention stays flat.
  (assign sink @[])
  (assign i (%add i 1)))

(assert (%gt k 0) "control: plain native tail read mis-read (harness broken)")

(assert (%gt a 0) "moved argument freed under the callee that owns it")
(assert (%gt b 0) "moved argument freed beside a hoisted sibling")
(assert (%gt c 0) "per-call callee closure freed under the frame it replaces")
(assert (%gt d 0) "captured value freed under the tail callee's read")
(assert (%gt e 0) "mutable accumulator freed under the walker that fills it")
(assert (%gt e2 0) "accumulator freed before the captured callee handed it back")
(assert (%gt e3 0) "accumulator freed under the caller's read of what it holds")
(assert (%gt e5 0) "moved-in accumulator freed before the callee minted for it")
(assert (%gt e6 0) "arm hand-back freed before the arm's callee minted for it")
(assert (%gt e7 0) "env cell freed under the callee that rewrites it")
(assert (%gt e8 0) "env cell freed under the arm's callee that rewrites it")
(assert (> e9 0) "env cell freed under a closure that escaped holding it")
(assert (> e10 0)
        "env cell freed under the closure the compensated arm hands out")
(assert (%gt e11 0)
        "cell content freed by the box release on the compensated arm")
(assert (%gt e12 0)
        "cell content freed by the reading arm's box release before its return")
(assert (%gt e13 0)
        "env cell freed under a closure that escaped before the reading arm ran")
(assert (%gt e14 0)
        "cell content freed under the reading arm's own opcode-read borrow")
(assert (%gt e15 0) "hand-back freed at an arm whose callee cannot reach it")
(assert (%gt e16 0) "fold accumulator freed under the combiner handed it next")
(assert (%gt e16b 0)
        "fold accumulator freed before its own combiner minted for it")
(assert (%gt e17 0)
        "a letrec closure's replicated release freed more than the frame's own \
         reference under a branch body tail")
(assert (%gt e18 0)
        "a scrutinee's replicated release freed more than the frame's own \
         reference at a merge inherited from the branch's entry")
(assert (> e4 0) "value freed under a closure that escaped holding it")
(assert (%gt f 0) "stranded value freed after being stored into a container")
(assert (%gt g 0) "returned value freed under the caller's read")
(assert (> h 0) "argument freed under a parked frame's resume")

(assert (%gt ai 0) "argument freed under the arm's callee that owns it")
(assert (%gt aj 0) "captured value freed under the arm callee's read")
(assert (%gt ak 0)
        "value released twice where the arm falls through to the merge")

(assert (%gt l 0)
        "forward cell freed under the deref that dispatches its sibling")
(assert (%gt m2 0) "forward cell freed before the handed-back capturer drove it")
(assert (> n2 0) "sibling freed under the caller that received it from its cell")
(assert (%gt s1 0)
        "arena freed under the sibling callee stranded by the same tail call")
(assert (%gt t1 0) "letrec member freed under the tail call that entered it")

(assert (%gt o1 0)
        "argument freed under the callee its own nested call handed it to")
(assert (%gt p1 0) "container freed under the element read out of it")
(assert (%gt q1 0) "container freed under an opcode read's borrow")
(assert (%gt r1 0) "capture freed under the lambda argument that holds it")

(println "region-tail-frame-exit-uaf: ok")
