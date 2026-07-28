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
# argument names. Witness (e2) is the one that fails without it.
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

# ── controls: the same reads with a NATIVE tail call — correct now ────────────
(defn c-plain (i)
  (let [x (list (string "p" i) i)]
    (length (first x))))

# ── drive: fresh subject each iteration; an over-early free faults on the read ─
(var i 0)
(var a 0)
(var b 0)
(var c 0)
(var d 0)
(var e 0)
(var e2 0)
(var f 0)
(var g 0)
(var h 0)
(var k 0)
(while (%lt i 3000)
  (assign a (a-moved i))
  (assign b (b-moved-beside i))
  (assign c (c-callee-local i))
  (assign d (d-captured i))
  (assign e (e-walker i))
  (assign e2 (e2-walker i))
  (assign f (f-read i))
  (assign g (g-return i))
  (assign h (h-fiber i))
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
(assert (%gt f 0) "stranded value freed after being stored into a container")
(assert (%gt g 0) "returned value freed under the caller's read")
(assert (> h 0) "argument freed under a parked frame's resume")

(println "region-tail-frame-exit-uaf: ok")
