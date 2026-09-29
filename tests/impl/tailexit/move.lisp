(elle/epoch 12)
# audited: 2026-09-08
# The witnesses a tail call reaches in its OWN block: what the move must not free, and the counted edges that hold each one.
#
# docs/impl/region/relocate.md
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
