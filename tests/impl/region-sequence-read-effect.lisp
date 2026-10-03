(elle/epoch 12)
# The sequence reads and conversions are read-only trait dispatchers
# (docs/impl/region/effects.md "Native region effects: declared, not guessed",
# the `Opaque` variant).
#
# `first`, `second`, `rest`, `->array` and `->list` each resolve their work
# through `Sequence`/`Collection`, so the RESULT is unbounded: `with-traits` may
# replace the protocol with a user closure returning anything, and each of them
# hands back an element of arg0, arg0 itself, or a fresh collection built from
# it. The STORE side is bounded regardless — the built-in method reads and
# copies, and a user closure is ordinary Elle code, which stores only through
# the runtime-counted mutable-store funnel. Two properties, two answers:
# unbounded result, no store — `Opaque`.
#
# What the wrong declaration costs is NOT the arg clique. Each of these takes a
# single heap argument, and the clique is over PAIRS of arguments, so there is no
# edge to emit either way. The cost is on the ESCAPE side, which reads the same
# declaration: `Mixed`/`Unknown` seeds every argument on escape's store facet
# (docs/impl/escape.md), and a region escaping by a facet other than return keeps
# the conservative baseline at every mechanism gated on `frame_held_regions`
# — the branch-arm release window among them.
#
# So the gauge below is a BRANCH: the subject is live-in, one arm reads it
# through a sequence read, and a sibling arm names it too. Where the read seeds a
# store facet on the subject the window declines, the branch's only release stays
# in the arm holding the `decref_point`, and the arm driven here strands the whole
# subject once per call. The `length`/`get` rows are the contrast — `Immediate`
# and `Funnel` seed nothing — and the single-arm rows are the control that the
# reads themselves reclaim.
#
# The soundness complement is region-sequence-read-effect-uaf.lisp: the reads
# still hand back a value living inside their argument, so the container-read
# borrow accounting must keep holding it up.

(def window 2000)

(defn measure (thunk warm window)
  (var i 0)
  (while (%lt i warm)
    (thunk)
    (assign i (%add i 1)))
  (def before (arena/count))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  (%sub (arena/count) before))

# subjects: one arm reads the live-in subject through a sequence read, a sibling
# arm names it too, and the driver takes the arm that is not the last to name it.
(defn arm-first (v t)
  (match t
    :a (first v)
    :b (length v)
    _ (length v)))
(defn arm-second (v t)
  (match t
    :a (second v)
    :b (length v)
    _ (length v)))
(defn arm-rest (v t)
  (match t
    :a (rest v)
    :b (length v)
    _ (length v)))
(defn arm-to-array (v t)
  (match t
    :a (->array v)
    :b (length v)
    _ (length v)))
(defn arm-to-list (v t)
  (match t
    :a (->list v)
    :b (length v)
    _ (length v)))

# an ARRAY subject, whose whole payload is one region: the seed is a claim about
# the argument, not about how many regions its value carries.
(defn arm-first-array (v t)
  (match t
    :a (first v)
    :b (length v)
    _ (length v)))

# contrast: `length` is `Immediate` and `get` is `Funnel`, so neither seeds the
# store facet and the same branch is bounded whatever the fix does. A red subject
# above beside a green row here isolates the declaration rather than the branch.
(defn arm-get (v t)
  (match t
    :a (get v 0)
    :b (length v)
    _ (length v)))
(defn arm-length (v t)
  (match t
    :a (length v)
    :b (length v)
    _ (length v)))

# control: the read with nothing to strand — a single arm naming the subject.
(defn ctl-first (v)
  (first v))

(def arm-first-d (measure (fn () (arm-first (list 1 2 3) :a)) 200 window))
(def arm-second-d (measure (fn () (arm-second (list 1 2 3) :a)) 200 window))
(def arm-rest-d (measure (fn () (arm-rest (list 1 2 3) :a)) 200 window))
(def arm-to-array-d (measure (fn () (arm-to-array (list 1 2 3) :a)) 200 window))
(def arm-to-list-d (measure (fn () (arm-to-list (list 1 2 3) :a)) 200 window))
(def arm-first-array-d (measure (fn () (arm-first-array [1 2 3] :a)) 200 window))
(def arm-get-d (measure (fn () (arm-get (list 1 2 3) :a)) 200 window))
(def arm-length-d (measure (fn () (arm-length (list 1 2 3) :a)) 200 window))
(def ctl-first-d (measure (fn () (ctl-first (list 1 2 3))) 200 window))

(println "region-sequence-read-effect deltas over " window " iters:")
(println "  first " arm-first-d "  second " arm-second-d "  rest " arm-rest-d)
(println "  ->array " arm-to-array-d "  ->list " arm-to-list-d "  first/array "
         arm-first-array-d)
(println "  contrast: get " arm-get-d "  length " arm-length-d "  control "
         ctl-first-d)

# Every leak here is at least one whole region per call, so a surviving strand
# reads >= 2000 over the window. 100 is slack for the one-time intercept.
(defn bounded? (d label)
  (assert (%lt d 100) (concat label " leaks, delta=" (number->string d))))

(bounded? ctl-first-d "control: a single-arm sequence read")
(bounded? arm-get-d "contrast: `get` declares Funnel")
(bounded? arm-length-d "contrast: `length` declares Immediate")

(bounded? arm-first-d "an arm reading the live-in subject with `first`")
(bounded? arm-second-d "an arm reading the live-in subject with `second`")
(bounded? arm-rest-d "an arm reading the live-in subject with `rest`")
(bounded? arm-to-array-d "an arm converting the live-in subject with `->array`")
(bounded? arm-to-list-d "an arm converting the live-in subject with `->list`")
(bounded? arm-first-array-d "an arm reading a live-in ARRAY with `first`")

# Value preservation: a declaration changes accounting, never the answer.
(assert (= (arm-first (list 1 2 3) :a) 1) "first arm result lost")
(assert (= (arm-second (list 1 2 3) :a) 2) "second arm result lost")
(assert (= (length (arm-rest (list 1 2 3) :a)) 2) "rest arm result lost")
(assert (= (length (arm-to-array (list 1 2 3) :a)) 3) "->array arm result lost")
(assert (= (length (arm-to-list (list 1 2 3) :a)) 3) "->list arm result lost")
(assert (= (arm-first-array [1 2 3] :a) 1) "first/array arm result lost")
(assert (= (arm-get (list 1 2 3) :a) 1) "get arm result lost")
(assert (= (ctl-first (list 1 2 3)) 1) "control read result lost")

(println "region-sequence-read-effect: ok")
