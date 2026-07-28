(elle/epoch 12)
# Soundness complement of region-branch-arm-window.lisp: re-anchoring a release
# out of a branch arm must not free anything early.
#
# The close moves a region's single release from inside one arm to the branch's
# consuming node (docs/impl/region/mechanism.md § "A release inside one arm is
# not a release on the other arms"). Moving a release LATER can only over-keep —
# but only while the frame is the region's sole holder when it runs, and only
# while the anchor is a point the arm actually reaches. The ways that fails all
# fault here, and each is a shape the admission must DECLINE, a boundary must
# stop, or a counted edge must survive: the arm handed the value to a container /
# a closure / its caller, so a second holder exists the moved release must leave
# standing — and where that holder ESCAPES, the admission refuses the window
# outright; the arm re-allocates per iteration of a nested loop, so
# one release cannot cover N; the arm's releases belong to another frame; the arm
# parked a fiber that resolves the region through its own activation map after the
# branch; and the arm leaves through a frame-replacing callee that never reaches
# the merge. A wrongly-admitted window frees a live region and the read below
# faults.
#
# Every witness reads the subject's HEAP contents after the branch, through a
# chain long enough that an over-early free faults rather than reading stale but
# still-mapped bytes. A fresh subject per iteration keeps region ids churning so
# a freed region is recycled under the reader.

# ── witnesses: a value live-in to a branch survives its later use ─────────────

# (a) the branch's value IS the subject, read after the branch. Every arm names
# it, so the moved release must land after the read that consumes the branch.
(defn w-result (i t)
  (let [r (match t
            :a (list (string "p" i) i)
            :b (list (string "q" i) i)
            _ (list (string "r" i) i))]
    (length (first r))))

# (b) the arm STORES the subject into a container that outlives the frame, and it
# is read back out afterwards. The store's incref is what the moved release must
# balance against — releasing the frame's reference is correct, freeing the
# stored value is not.
(def @sink @[])
(defn w-store (v t)
  (match t
    :a (push sink v)
    :b (push sink v)
    _ 0)
  (length (get sink (%sub (length sink) 1))))

# (c) the arm RETURNS the subject while a later sibling holds the `decref_point`:
# the caller's read must see it alive, so the moved release must land after the
# return mint, not before it.
(defn w-return-inner (v t)
  (match t
    :a v
    :b (first v)
    _ v))
(defn w-return (v t)
  (length (first (w-return-inner v t))))

# (d) the arm hands the subject to a CLOSURE that is called in place. The window
# admits the holder — the funnel counted the env's hold when the closure was built
# — so the release does move to the merge, and what must stand is that count: the
# closure reads through `v` after it.
(defn w-capture (v t)
  (let [f (match t
            :a
              (fn () (length (first v)))
            :b
              (fn () (length (first v)))
            _ (fn () 0))]
    (f)))

# (d2) the same capture, by a closure that ESCAPES — it is returned, so it
# outlives this activation and carries `v` with it. Escape's capture facet refuses
# the holder, the in-arm release stands, and the caller's later invocation reads
# through it.
(defn w-escaping (v t)
  (match t
    :a
      (fn () (length (first v)))
    :b
      (fn () (length (first v)))
    _ (fn () 0)))
(defn w-escaping-read (v t)
  ((w-escaping v t)))

# (e) a nested LOOP inside the arm: each iteration allocates its own value and
# reads it. Releasing those at the branch's merge instead would free one region
# for eight allocations, every earlier iteration's value recycled under the next
# read.
(defn w-loop (i t)
  (match t
    :a
      (begin
        (var k 0)
        (var n 0)
        (while (%lt k 8)
          (let [x (list (string "l" i "-" k) k)]
            (assign n (%add n (length (first x)))))
          (assign k (%add k 1)))
        n)
    :b 1
    _ 2))

# (f) a nested LAMBDA inside the arm, called repeatedly: its body's releases
# belong to the closure's activation. Hoisting one to the enclosing branch would
# release a region resolved against the wrong frame's slot.
(defn w-lambda (i t)
  (match t
    :a
      (let [f (fn (j)
                (let [x (list (string "m" i "-" j) j)]
                  (length (first x))))]
        (%add (f 1) (f 2)))
    :b 1
    _ 2))

# (g) the arm PARKS a fiber that resolves the subject through its own activation
# map, and the fiber is resumed after the branch has released. This is the
# uncounted-borrow hazard the per-arm route cannot discharge; the moved release
# must still leave the parked frame's view alive.
(defn w-park (v t)
  (let [f (match t
            :a
              (fiber/new (fn ()
                           (yield (length (first v)))
                           (length (first v))) |:yield|)
            :b
              (fiber/new (fn () (length (first v))) |:yield|)
            _ (fiber/new (fn () 0) |:yield|))]
    (fiber/resume f)
    (fiber/resume f)))

# (h) an arm that leaves through a frame-replacing TAIL CALL, with a sibling arm
# naming the same parameter. The branch declines the window, so this arm's own
# release must still run where it was placed.
(defn w-tail-callee (v)
  (length (first v)))
(defn w-tail (v t)
  (match t
    :a (length (first v))
    :b (w-tail-callee v)
    _ 0))

# (i) the `If` face, with the subject consumed after the branch.
(defn w-if (v c)
  (let [r (if c (first v) (last v))]
    (length r)))

# ── controls: the same reads through a single arm — correct now ───────────────
(defn c-plain (v)
  (length (first v)))

# ── drive: fresh subject each iteration; an over-early free faults on the read ─
(var i 0)
(var a 0)
(var b 0)
(var c 0)
(var d 0)
(var e 0)
(var f 0)
(var g 0)
(var h 0)
(var j 0)
(var k 0)
(var m 0)
(while (%lt i 3000)
  (assign a (w-result i :a))
  (assign b (w-store (list (string "s" i) i) :a))
  (assign c (w-return (list (string "c" i) i) :a))
  (assign d (w-capture (list (string "d" i) i) :a))
  (assign m (w-escaping-read (list (string "n" i) i) :a))
  (assign e (w-loop i :a))
  (assign f (w-lambda i :a))
  (assign g (w-park (list (string "g" i) i) :a))
  (assign h (w-tail (list (string "h" i) i) :b))
  (assign j (w-if (list (string "j" i) (string "jj" i)) true))
  (assign k (c-plain (list (string "k" i) i)))
  # The sink is a module-level container by design (witness b stores into it);
  # drain it so the driver's own retention stays flat.
  (assign sink @[])
  (assign i (%add i 1)))

(assert (%gt k 0) "control: single-arm read mis-read (harness broken)")

(assert (%gt a 0) "branch result freed under the post-branch read")
(assert (%gt b 0) "arm value freed after being stored into a container")
(assert (%gt c 0) "arm value freed under the caller's read of the return")
(assert (> d 0) "arm value freed under the closure that captured it")
(assert (> m 0) "arm value freed under a closure that escaped holding it")
(assert (%gt e 0) "loop-body value freed under a later iteration's read")
(assert (%gt f 0) "lambda-body value released from the enclosing frame")
(assert (> g 0) "parked fiber's borrow freed by the moved release")
(assert (%gt h 0) "tail-call arm's own release lost to the merge")
(assert (%gt j 0) "`if` arm value freed under the post-branch read")

(println "region-branch-arm-window-uaf: ok")
