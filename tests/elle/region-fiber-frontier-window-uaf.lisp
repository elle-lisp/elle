(elle/epoch 12)
# Soundness complement of region-fiber-frontier-window.lisp: admitting the fiber
# facet must not free anything early.
#
# The admission rests on one fact — every seam that hands a value to another fiber
# counts a reference of its own, so the crossing leaves a COUNTED holder rather
# than the uncounted borrow the frame-held question guards against
# (docs/impl/region/mechanism.md § "A fiber crossing is a counted holder too").
# Going out, the park's `EmitEscape` retain is that reference. Coming back, the
# resume value's own mint is (docs/impl/region/owner.md § "A resume value crosses
# counted, or not at all"): the resumer pushes the value onto the parked frame's
# stack and takes nothing for it, so without the mint the body reads the resumer's
# reference — fine while the resume is still running, and a dangling read the
# moment the body parks again holding the value and the resumer moves on.
#
# So each witness below drives a delivered or emitted value across a point where
# the OTHER side has already released: a body that keeps the value past a further
# park, two bodies that keep the same value, a resumer that reads what it was
# yielded after the emitting body ran on, and a value delivered from inside a
# branch arm whose release the window now anchors at the merge. Beside them is the
# refusal the admission must still make — a subject a containment facet carries to
# a holder nothing counts — which must keep its in-arm release.
#
# Every witness reads the subject's HEAP contents through a chain long enough that
# an over-early free faults rather than reading stale but still-mapped bytes, and
# a fresh subject per iteration keeps region ids churning so a freed region is
# recycled under the reader.

# ── witnesses ─────────────────────────────────────────────────────────────────

# A body that keeps its resume value across a FURTHER park: the resumer's own
# release has run by the time the second read happens, so only the value's own
# mint can be holding it.
(defn keeper-body ()
  (let [x (yield 0)]
    (yield (length (first x)))
    (length (first x))))

(defn wake-all (ws v)
  (each w in ws
    (fiber/resume w v)))

(defn w-keep (i)
  (let [w (fiber/new keeper-body |:yield|)]
    (fiber/resume w)
    (wake-all (array w) (list (string "k" i) i))
    (fiber/resume w)))

# TWO bodies keeping the same delivered value: one delivery reference per crossing,
# so a model that moves a single reference along leaves the second body reading a
# value the first already gave back.
(defn w-keep-two (i)
  (let [a (fiber/new keeper-body |:yield|)
        b (fiber/new keeper-body |:yield|)]
    (fiber/resume a)
    (fiber/resume b)
    (wake-all (array a b) (list (string "t" i) i))
    (let [ra (fiber/resume a)
          rb (fiber/resume b)]
      (if (and (number? ra) (number? rb)) (+ ra rb) 0))))

# The emit direction: the resumer reads what it was handed after the emitting body
# has run on past the yield and released its own reference.
(defn yielder (v)
  (yield v)
  0)
(defn w-emit (i)
  (let [f (fiber/new (fn () (yielder (list (string "e" i) i))) |:yield|)]
    (let [got (fiber/resume f)]
      (fiber/resume f)
      (length (first got)))))

# The delivery sits in a BRANCH ARM, so the subject's one release is anchored at
# the merge the window now admits. The arm that delivers must leave the receiver's
# count standing; the arm that does not must still release. Both are driven.
(defn arm-deliver (w v t)
  (match t
    :a (length (first v))
    _ (fiber/resume w v)))
(defn w-arm-cross (i)
  (let [w (fiber/new keeper-body |:yield|)]
    (fiber/resume w)
    (arm-deliver w (list (string "c" i) i) :z)
    (fiber/resume w)))
(defn w-arm-short (i)
  (let [w (fiber/new keeper-body |:yield|)]
    (fiber/resume w)
    (arm-deliver w (list (string "s" i) i) :a)))

# ── the refusal the admission must still make ─────────────────────────────────
# A subject a CONTAINMENT facet carries to a holder nothing counts: the arm stores
# it into a container that outlives the frame, and it is read back out afterwards.
# The window must decline that region and leave the in-arm release standing.
(def @sink @[])
(defn w-store (v t)
  (match t
    :a (push sink v)
    :b (push sink v)
    _ 0)
  (length (first (get sink (%sub (length sink) 1)))))

# ── control: the same reads with no crossing — correct now ────────────────────
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
(while (%lt i 2000)
  (assign a (w-keep i))
  (assign b (w-keep-two i))
  (assign c (w-emit i))
  (assign d (w-arm-cross i))
  (assign e (w-arm-short i))
  (assign f (w-store (list (string "p" i) i) :a))
  (assign g (c-plain (list (string "q" i) i)))
  # The sink is a module-level container by design (witness f stores into it);
  # drain it so the driver's own retention stays flat.
  (assign sink @[])
  (assign i (%add i 1)))

(assert (> g 0) "control: single-arm read mis-read (harness broken)")
(assert (> a 0) "delivered value freed under the body that kept it past a park")
(assert (> b 0) "delivered value freed under the second body that kept it")
(assert (> c 0) "yielded value freed under the resumer's read")
(assert (> d 0)
        "delivered value freed by the merge release its delivering arm admitted")
(assert (> e 0)
        "live-in subject freed by the merge release its sibling arm admitted")
(assert (> f 0) "stored subject freed though the window must refuse it")

(println "region-fiber-frontier-window-uaf: ok")
