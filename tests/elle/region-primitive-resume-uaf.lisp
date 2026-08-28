(elle/epoch 12)
# A resume value delivered into a parked PRIMITIVE call carries one owning
# reference (docs/impl/region/owner.md § "A delivery into a replayed frame
# carries one owning reference").
#
# A parked frame re-enters at its suspending call's continuation, and that
# continuation runs the call's compiler-emitted result release. An ordinary
# callee funds the reference that release consumes with its `Return` mint. A
# primitive that SUSPENDS never returns at all: the resume value takes the place
# of its result, so unless the delivery mints one, the continuation releases a
# reference the resumer still owns and the value dies under every holder that
# outlives the resume.
#
# Two parks land in that continuation. A dynamic `emit` — a non-literal first
# argument, which falls through to the runtime primitive — and a capability
# denial, which parks any denied primitive call for the parent to mediate. The
# literal `emit` is a block terminator whose resume block mints the reference in
# bytecode, so it is the control each witness is paired against: the same
# program, the same reads, one path already correct.
#
# Each witness hands the resume a FRESH string the resumer releases as soon as
# the call returns, then compares the delivered content against an equal string
# built afterwards. Where the freed page has already been recycled the compare
# reports the corruption on any run; where it has not, the read is stale but
# still mapped — so this file is also pinned under `--trace=guardfree`
# (`region_primitive_resume_uaf`), which unmaps the page and faults on it. A
# fresh subject per iteration keeps region ids churning under both oracles.

(def sig :yield)

# ── (a) the body binds the resume value and builds from it ───────────────────
# The array the body returns points into the resumer's region, so the resumer's
# own release of the delivered string must not be the last one.
(defn w-bind (i)
  (let [f (fiber/new (fn ()
                       (let [r (emit sig 7)]
                         [:resumed r])) |:yield|)]
    (fiber/resume f)
    (let [out (fiber/resume f (string "bind" i))]
      (if (= (get out 1) (string "bind" i)) 1 0))))

# ── (b) the body RETURNS the resume value from tail position ─────────────────
# The dynamic emit is the body's tail call, so the delivered value becomes the
# fiber's terminal result and is read back out of the resume.
(defn w-tail (i)
  (let [f (fiber/new (fn () (emit sig 7)) |:yield|)]
    (fiber/resume f)
    (if (= (fiber/resume f (string "tail" i)) (string "tail" i)) 1 0)))

# ── (c) the body keeps the resume value across a FURTHER park ────────────────
# The resumer's release has run by the time the second read happens, so only the
# delivery's own reference can still be holding the value.
(defn w-keep (i)
  (let [f (fiber/new (fn ()
                       (let [r (emit sig 0)]
                         (emit sig (length r))
                         (first r))) |:yield|)]
    (fiber/resume f)
    (fiber/resume f (string "keep" i))
    (if (= (fiber/resume f) (first (string "keep" i))) 1 0)))

# ── (d) a CAPABILITY DENIAL parks the call the parent mediates ───────────────
# `:deny |:fs|` suspends `file/read` before it runs; the parent answers the read
# itself and resumes with the answer, which the child then reads back.
(defn w-denied (i)
  (let [f (fiber/new (fn ()
                       (let [r (file/read "/nonexistent-mediated")]
                         [:mediated r])) |:fs :error| :deny |:fs|)]
    (fiber/resume f)
    (let [out (fiber/resume f (string "denied" i))]
      (if (= (get out 1) (string "denied" i)) 1 0))))

# ── controls: the literal path, which mints the reference in bytecode ────────
(defn c-bind (i)
  (let [f (fiber/new (fn ()
                       (let [r (emit :yield 7)]
                         [:resumed r])) |:yield|)]
    (fiber/resume f)
    (let [out (fiber/resume f (string "cbind" i))]
      (if (= (get out 1) (string "cbind" i)) 1 0))))

(defn c-tail (i)
  (let [f (fiber/new (fn () (emit :yield 7)) |:yield|)]
    (fiber/resume f)
    (if (= (fiber/resume f (string "ctail" i)) (string "ctail" i)) 1 0)))

# ── drive: a fresh subject each iteration; an over-early free faults on it ───

(defn drive (reps)
  (var i 0)
  (var a 0)
  (var b 0)
  (var c 0)
  (var d 0)
  (var e 0)
  (var f 0)
  (while (%lt i reps)
    (assign a (w-bind i))
    (assign b (w-tail i))
    (assign c (w-keep i))
    (assign d (w-denied i))
    (assign e (c-bind i))
    (assign f (c-tail i))
    (assign i (%add i 1)))
  (list a b c d e f))

(let [r (drive 800)]
  (assert (= (get r 4) 1) "control: literal emit bind mis-read (harness broken)")
  (assert (= (get r 5) 1) "control: literal emit tail mis-read (harness broken)")
  (assert (= (get r 0) 1)
          "dynamic emit: resume value freed under the body that bound it")
  (assert (= (get r 1) 1)
          "dynamic emit: resume value freed under the tail result it became")
  (assert (= (get r 2) 1)
          "dynamic emit: resume value freed under the body that kept it past a park")
  (assert (= (get r 3) 1)
          "capability denial: mediated value freed under the resumed body"))

# The leak face — that the delivery mints exactly one reference per park — is the
# `primitive-resume-bind`/`-tail`/`-keep` probes in tests/elle/oracle.lisp, read
# against the `emit-resume-literal` control. Each measures a per-op rate with a
# confidence interval rather than comparing two heap samples: a surplus mint
# strands the resume value per park, and a two-point integer delta floors a
# sub-integer rate to zero.

(println "region-primitive-resume-uaf: ok")
