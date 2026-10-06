(elle/epoch 14)
# audited: 2026-10-06
# Declaring `fiber/child` and `import/load-file` `Opaque` frees no argument before its last reader.
# docs/impl/region/effects.md
# docs/impl/escape.md
#
# Soundness complement of region-fiber-child-effect.lisp. The declaration says
# two things and no more: the result may live anywhere (so the walk keeps
# recording `result ⊒ each argument`), and no argument is stored uncounted (so
# escape seeds nothing on its store facet). What that withdraws is the false
# store facet, and with it the refusal it forced on every mechanism gated on
# `frame_held_regions` — the branch-arm release window among them.
#
# So the hazards are the ones the withdrawn refusal used to mask, and they are
# about the ARGUMENT: its release now lands at the branch merge rather than inside
# the arm that names it last, so it must still land behind every later reader. The
# witnesses drive a fiber read after the branch, resumed after it, stored into a
# container a sibling arm reads, returned to a caller that resumes it, captured by
# a closure called later, and held across the fiber frontier — plus the same shapes
# for the path `import/load-file` reads. Each read reaches the subject's own pages
# (`fiber/bits` reads the fiber object; a resume runs its body out of them), so an
# over-early free faults rather than reading stale but mapped bytes. A fresh
# subject per iteration keeps region ids churning.

# ── the fiber subject ────────────────────────────────────────────────────────

# (a) the subject is read again after the branch. The window moves the fiber's
# release to the merge, which must still land behind this second read.
(defn w-read-after (f t)
  (fiber/resume f)
  (match t
    :a (fiber/child f)
    :b (fiber/bits f)
    _ (fiber/bits f))
  (fiber/bits f))

# (b) the subject is RESUMED after the branch — the deepest read of a fiber there
# is, running its body out of its own pages.
(defn w-resumed-after (f t)
  (match t
    :a (fiber/child f)
    :b (fiber/bits f)
    _ (fiber/bits f))
  (length (fiber/resume f)))

# (c) the subject is STORED into a container outliving the frame by one arm while
# another arm reads it. The store is a real escape facet, so the window still
# refuses and the read back out must find the fiber alive.
(def @sink @[])
(defn w-stored (f t)
  (fiber/resume f)
  (match t
    :a (push sink f)
    :b (fiber/child f)
    _ 0)
  (fiber/bits (get sink (%sub (length sink) 1))))

# (d) the subject ESCAPES — it is returned, and the caller resumes it.
(defn w-escaped (f t)
  (match t
    :a (begin
         (fiber/child f)
         f)
    :b f
    _ f))
(defn w-escaped-outer (f t)
  (let [g (w-escaped f t)]
    (length (fiber/resume g))))

# (e) the subject is captured by a closure called AFTER the branch, so the
# capture's read outlives the merge.
(defn w-captured (f t)
  (fiber/resume f)
  (let [peek (fn () (fiber/bits f))]
    (match t
      :a (fiber/child f)
      :b (fiber/bits f)
      _ (fiber/bits f))
    (peek)))

# (f) the subject crosses a FIBER boundary — an inner fiber captures it, reads it
# in a branch arm, and suspends holding it, so the resume below must find a fiber
# two activations still name.
(defn w-frontier (f t)
  (let [inner (fiber/new (fn ()
                           (match t
                             :a (fiber/child f)
                             :b (fiber/bits f)
                             _ (fiber/bits f))
                           (yield (fiber/bits f))
                           0) |:yield|)]
    (fiber/resume inner)
    (length (fiber/resume f))))

# ── the loader's path ───────────────────────────────────────────────────────

# (g) the path is read after the branch that loaded with it. The load fails by
# design — the loader copies the path out to a Rust String either way, which is
# the claim under test — and the caught error is data.
(defn w-path-after (s t)
  (match t
    :a (try
         (import/load-file s)
         (catch e 0))
    :b (length s)
    _ (length s))
  (length (concat s "!")))

# (h) the path is stored into a container a later read reaches, so its release
# must land behind that read.
(def @paths @[])
(defn w-path-stored (s t)
  (match t
    :a
      (begin
        (try
          (import/load-file s)
          (catch e 0))
        (push paths s))
    :b (push paths s)
    _ 0)
  (length (concat (get paths (%sub (length paths) 1)) "!")))

# ── controls: the same reads with no branch — correct now ────────────────────
(defn c-plain (f)
  (fiber/child f)
  (length (fiber/resume f)))

# ── drive: fresh subject each iteration; an over-early free faults on the read ─
(defn fresh-fiber (n)
  (fiber/new (fn ()
               (yield (string "y" n))
               0) |:yield|))

(var i 0)
(var a 0)
(var b 0)
(var c 0)
(var d 0)
(var e 0)
(var g 0)
(var h 0)
(while (%lt i 1500)
  (assign a (w-read-after (fresh-fiber i) :a))
  (assign b (w-resumed-after (fresh-fiber i) :a))
  (assign c (w-stored (fresh-fiber i) :a))
  (assign d (w-escaped-outer (fresh-fiber i) :a))
  (assign e (w-captured (fresh-fiber i) :a))
  (assign g (w-frontier (fresh-fiber i) :a))
  (assign h (c-plain (fresh-fiber i)))
  # The sink is a module-level container by design (witness c stores into it);
  # drain it so the driver's own retention stays flat.
  (assign sink @[])
  (assign i (%add i 1)))

(assert (%gt h 0) "control: single-arm read mis-read (harness broken)")
(assert (> a 0) "fiber freed under a second read after the branch")
(assert (%gt b 0) "fiber freed under a resume after the branch")
(assert (> c 0) "stored fiber freed by a sibling arm's read")
(assert (%gt d 0) "returned fiber freed under the caller's resume")
(assert (> e 0) "fiber freed under a capture called after the branch")
(assert (%gt g 0) "fiber freed under a read across the fiber frontier")

# The path witnesses run a shorter loop: every call asks the filesystem for the
# file, which is orders of magnitude slower than the fiber reads above. A fresh
# path per iteration still churns the region ids.
(var j 0)
(var p 0)
(var q 0)
(while (%lt j 300)
  (assign p (w-path-after (string "nope/missing" j) :a))
  (assign q (w-path-stored (string "nope/missing" j) :a))
  (assign paths @[])
  (assign j (%add j 1)))

(assert (%gt p 0) "path freed under a read after the branch")
(assert (%gt q 0) "stored path freed under the read back out")

(println "region-fiber-child-effect-uaf: ok")
