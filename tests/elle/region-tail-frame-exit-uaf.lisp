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
# proving the frame is the region's sole holder — covers the path the exemption
# cannot see: a tail callee also reaches its CAPTURED environment, which no
# argument names — but the env's hold is the funnel's counted edge, so that path
# is admitted rather than refused, and (e3) is the row that proves the count
# stands. A value the callee hands BACK rides that same edge one step further —
# the callee's `Return` mints the caller's reference after the relocated release
# has run, and the env edge is what holds the region off zero in between, which
# (e2), (e5) and (e6) drive. (e7) and (e8) drive the same admission for an ENV
# CELL's `DecrefCellRegion`, whose holder is REASSIGNED: that refusal is about a
# release routed through the holder's slot, and this one names the cell box no
# `assign` repoints. What the admission still refuses is a holder escape marks by a
# facet no edge at the point replaces: a closure that leaves carrying it (e4, and
# e9 for the cell), a store into a longer-lived container (f), a fiber crossing
# (h).
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

# (e9) the closure holding the reassigned cell ESCAPES, so escape's capture facet
# refuses the holder and the box must stay in the dead block: the returned closure
# rewrites and reads its cell after the frame is gone.
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

# (e4) the capturing closure ESCAPES — it is returned, so it outlives the frame
# and carries `x` with it. Escape's capture facet refuses the holder, and the
# release must stay in the dead block; the caller invokes the closure afterwards
# and reads through it.
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

# (m) the CAPTURER is handed back, so the sole-held admission refuses and the return
# half admits on the callee's captured edge — which is `closure ⊇ cell`. The caller
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
(var o1 0)
(var p1 0)
(var q1 0)
(var r1 0)
(while (%lt i 1500)
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

(assert (%gt o1 0)
        "argument freed under the callee its own nested call handed it to")
(assert (%gt p1 0) "container freed under the element read out of it")
(assert (%gt q1 0) "container freed under an opcode read's borrow")
(assert (%gt r1 0) "capture freed under the lambda argument that holds it")

(println "region-tail-frame-exit-uaf: ok")
