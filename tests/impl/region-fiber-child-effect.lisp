(elle/epoch 12)
# `fiber/child` is a fiber-graph read: it stores nothing
# (docs/impl/region/effects.md "Native region effects: declared, not guessed",
# the `Opaque` variant, § "A fiber-graph read is `Opaque`").
#
# The call hands back the cached child-fiber `Value` its argument carries. That
# cache is written by the resume machinery (`with_child_fiber`), not by this
# call, so the read stores no argument; what it returns lives in whatever region
# the child was minted in, which is neither the call's own region nor its
# argument's. Unbounded result, no store — `Opaque`.
#
# What the wrong declaration costs is NOT the arg clique. `fiber/child` takes a
# single heap argument and the clique is over PAIRS of arguments, so there is no
# edge to emit either way. The cost is on the ESCAPE side, which reads the same
# declaration: `Mixed`/`Unknown` seeds every argument on escape's store facet
# (docs/impl/escape.md), and a region escaping by a facet other than return keeps
# the conservative baseline at every mechanism gated on `frame_held_regions` —
# the branch-arm release window among them.
#
# So the gauge below is a BRANCH: the fiber subject is live-in, one arm reads it
# with `fiber/child`, and a sibling arm names it too. Where the read seeds a
# store facet on the subject the window declines, the branch's only release stays
# in the arm holding the `decref_point`, and the arm driven here strands the whole
# subject — the fiber value and its body closure — once per call. The
# `fiber/bits` rows are the contrast (`Immediate` seeds nothing) and the
# single-arm row is the control that the read itself reclaims.
#
# `import` is the same declaration on the other face — a native that re-enters
# the VM, copies its specifier out to a Rust `String`, and hands back a value
# minted by the module's own compiled top level. It has no probe here because
# every call re-runs the module it names: its pins are the unit-level
# counterfactuals `import_declares_opaque_no_hard_edge` and
# `import_does_not_seed_the_store_facet`.

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

# subject: one arm reads the live-in fiber with `fiber/child`, a sibling arm
# names it too, and the driver takes the arm that is not the last to name it.
(defn arm-child (f t)
  (match t
    :a (fiber/child f)
    :b (fiber/bits f)
    _ (fiber/bits f)))

# an `if` reaches the same window through two arms rather than three.
(defn if-child (f t)
  (if t (fiber/child f) (fiber/bits f)))

# contrast: `fiber/bits` is `Immediate`, so it seeds nothing and the same branch
# is bounded whatever the declaration does. A red subject above beside a green
# row here isolates the declaration rather than the branch.
(defn arm-bits (f t)
  (match t
    :a (fiber/bits f)
    :b (fiber/bits f)
    _ (fiber/bits f)))

# control: the read with nothing to strand — a single arm naming the subject.
(defn ctl-child (f)
  (fiber/child f))

(defn fresh-fiber ()
  (fiber/new (fn () 1) |:error|))

(def arm-child-d (measure (fn () (arm-child (fresh-fiber) :a)) 200 window))
(def if-child-d (measure (fn () (if-child (fresh-fiber) true)) 200 window))
(def arm-bits-d (measure (fn () (arm-bits (fresh-fiber) :a)) 200 window))
(def ctl-child-d (measure (fn () (ctl-child (fresh-fiber))) 200 window))

(println "region-fiber-child-effect deltas over " window " iters:")
(println "  arm " arm-child-d "  if " if-child-d)
(println "  contrast: bits " arm-bits-d "  control " ctl-child-d)

# Every leak here is at least one whole region per call, so a surviving strand
# reads >= 2000 over the window. 100 is slack for the one-time intercept.
(defn bounded? (d label)
  (assert (%lt d 100) (concat label " leaks, delta=" (number->string d))))

(bounded? ctl-child-d "control: a single-arm fiber/child read")
(bounded? arm-bits-d "contrast: `fiber/bits` declares Immediate")

(bounded? arm-child-d "a match arm reading the live-in fiber with `fiber/child`")
(bounded? if-child-d "an if arm reading the live-in fiber with `fiber/child`")

# Value preservation: a declaration changes accounting, never the answer. A
# never-resumed fiber has no child, and the reads still answer about the subject.
(assert (nil? (arm-child (fresh-fiber) :a)) "fiber/child arm result lost")
(assert (nil? (if-child (fresh-fiber) true)) "fiber/child if-arm result lost")
(assert (nil? (ctl-child (fresh-fiber))) "fiber/child control result lost")
(assert (= (arm-bits (fresh-fiber) :a) 0) "fiber/bits contrast result lost")

(println "region-fiber-child-effect: ok")
