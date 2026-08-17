(elle/epoch 12)
# A fiber body owns one reference of every value it yields
# (docs/impl/region/owner.md § "Park/unpark symmetry").
#
# A park's `EmitEscape` retain is the DELIVERY reference: the resumer's
# compiler-emitted release of the resume result consumes it, exactly as a
# normally-completing child's `Return` mint funds the release of a terminal
# result. What the discard discharge (`release_discarded_signal`, reached from the
# region free's fiber discharge) stands in for is a DIFFERENT reference — the
# body's own, whose release lives in the continuation past the yield and never
# runs for a fiber abandoned while suspended.
#
# A payload the body ALLOCATED carries that second reference itself. A payload it
# merely borrows — a capture, a parameter, a module-level binding — carries none,
# so without a minted one the discharge releases the delivery reference the
# resumer already consumed and the value dies under every holder that outlives the
# fiber: the yielding frame's own binding, a container, the caller.
#
# The fault needs both ingredients, and the controls at the bottom remove one
# each: resume the fiber to completion, or yield something the body allocated, and
# the same program is balanced without the mint. So a fix that mints
# unconditionally trades this fault for a per-yield leak, which the growth gauge
# at the end refuses.
#
# Each witness reads its subject AFTER the abandoned fiber's region is gone —
# through the yielded value the resume handed back, through `fiber/value`, and
# through the borrow's own holder — so an over-early free faults rather than
# reading stale but mapped bytes. A fresh subject per iteration keeps region ids
# churning, so a recycled id detonates on its generation stamp.

# ── (a) the borrow is a PARAMETER of the yielding frame ──────────────────────
# The frame still holds `s` when the fiber is abandoned, so the discharge's decref
# must not be the last one.
(defn w-param (s)
  (let [inner (fiber/new (fn ()
                           (yield s)
                           0) |:yield|)]
    (let [n (length (fiber/resume inner))]
      (%add n (length s)))))

# ── (b) the borrow is a `let`-local of the yielding frame ────────────────────
(defn w-local (n)
  (let [s (string "local" n)]
    (let [inner (fiber/new (fn ()
                             (yield s)
                             0) |:yield|)]
      (let [m (length (fiber/resume inner))]
        (%add m (length s))))))

# ── (c) the borrow is a MODULE-level binding, read again after the drive ─────
(def shared (string "shared-subject"))
(defn w-module ()
  (let [inner (fiber/new (fn ()
                           (yield shared)
                           0) |:yield|)]
    (length (fiber/resume inner))))

# ── (d) the yielded value is read through `fiber/value`, not the resume ──────
# The documented generator idiom reads the payload off the fiber after each
# resume, so a generator abandoned mid-sequence takes this route.
(defn w-fiber-value (s)
  (let [inner (fiber/new (fn ()
                           (yield s)
                           0) |:yield|)]
    (fiber/resume inner)
    (let [n (length (fiber/value inner))]
      (%add n (length s)))))

# ── (e) the borrow outlives the yielding frame in a container ────────────────
# Nothing in the frame reads `s` after the resume; the holder that must still find
# it alive is the array the caller reads back out.
(def @sink @[])
(defn w-stored (s)
  (push sink s)
  (let [inner (fiber/new (fn ()
                           (yield s)
                           0) |:yield|)]
    (length (fiber/resume inner))))

# ── (f) two yields of the same borrow, the second one abandoned ──────────────
# Each park delivers a reference of its own, so the accounting must hold per park
# rather than per value.
(defn w-twice (s)
  (let [inner (fiber/new (fn ()
                           (yield s)
                           (yield s)
                           0) |:yield|)]
    (fiber/resume inner)
    (let [n (length (fiber/resume inner))]
      (%add n (length s)))))

# ── controls — remove one ingredient each; correct without a minted reference ─

# (g) the fiber is RESUMED TO COMPLETION, so the body's continuation runs.
(defn c-completed (s)
  (let [inner (fiber/new (fn ()
                           (yield s)
                           0) |:yield|)]
    (let [n (length (fiber/resume inner))]
      (fiber/resume inner)
      (%add n (length s)))))

# (h) the body ALLOCATES what it yields, so it owns the second reference.
(defn c-allocated (s)
  (let [inner (fiber/new (fn ()
                           (yield (concat s "!"))
                           0) |:yield|)]
    (let [n (length (fiber/resume inner))]
      (%add n (length s)))))

# (i) the body yields only a READ of the borrow — an immediate crosses no region.
(defn c-read (s)
  (let [inner (fiber/new (fn ()
                           (yield (length s))
                           0) |:yield|)]
    (fiber/resume inner)
    (length s)))

# (j) the fiber is never resumed at all, so nothing is ever delivered.
(defn c-unresumed (s)
  (let [inner (fiber/new (fn ()
                           (yield s)
                           0) |:yield|)]
    (fiber/bits inner)
    (length s)))

# ── drive: a fresh subject per iteration; an over-free faults on the read ─────

(defn drive [reps]
  (var i 0)
  (var a 0)
  (var b 0)
  (var c 0)
  (var d 0)
  (var e 0)
  (var f 0)
  (var g 0)
  (var h 0)
  (var k 0)
  (var l 0)
  (while (%lt i reps)
    (assign a (w-param (string "param" i)))
    (assign b (w-local i))
    (assign c (w-module))
    (assign d (w-fiber-value (string "value" i)))
    (assign e (w-stored (string "stored" i)))
    (assign f (w-twice (string "twice" i)))
    (assign g (c-completed (string "done" i)))
    (assign h (c-allocated (string "alloc" i)))
    (assign k (c-read (string "read" i)))
    (assign l (c-unresumed (string "cold" i)))
    # The witness (e) sink is a module-level container by design; read the stored
    # borrow back out — it must still be alive — then drain so the driver's own
    # retention stays flat.
    (assert (%gt (length (get sink (%sub (length sink) 1))) 0)
            "stored borrow freed by the abandoned fiber's discharge")
    (assign sink @[])
    (assign i (%add i 1)))
  (list a b c d e f g h k l))

(let [r (drive 800)]
  (assert (> (get r 0) 0) "parameter borrow freed under the yielding frame")
  (assert (> (get r 1) 0) "let-local borrow freed under the yielding frame")
  (assert (> (get r 2) 0) "module-level borrow freed by an abandoned fiber")
  (assert (> (get r 3) 0) "borrow freed under a `fiber/value` read")
  (assert (> (get r 4) 0) "stored borrow freed under the container read")
  (assert (> (get r 5) 0) "borrow freed by the second of two parks")
  (assert (> (get r 6) 0) "control: completed fiber mis-read (harness broken)")
  (assert (> (get r 7) 0) "control: body-allocated yield mis-read")
  (assert (> (get r 8) 0) "control: read-only yield mis-read")
  (assert (> (get r 9) 0) "control: never-resumed fiber mis-read"))

# The module-level subject must survive every abandoned fiber that yielded it.
(assert (%gt (length shared) 0)
        "module-level borrow freed by an abandoned fiber")

# ── the leak face: minting a reference at every yield would strand one per park ─
# The mint belongs only where the body owns none, so steady-state region growth
# stays flat across the whole witness set.
(drive 100)
(let [before (arena/region-count)]
  (drive 400)
  (let [growth (%sub (arena/region-count) before)]
    (assert (%lt growth 40)
            (string "yield-borrow accounting strands regions: live count grew by "
                    growth " over 400 iterations of six abandoned parks each "
                    "(expected flat)"))))

(println "region-fiber-yield-borrow-uaf: ok")
