(elle/epoch 12)
# A fiber body owns one reference of every value it yields, and what yields is the
# emit OPERATION rather than the `Emit` node (docs/impl/region/owner.md
# § "Park/unpark symmetry").
#
# A park's escape retain is the DELIVERY reference: the resumer's compiler-emitted
# release of the resume result consumes it. What a discarded fiber's discharge
# stands in for is the body's SEPARATE reference, released by the continuation past
# the suspend — a release a fiber abandoned while suspended never reaches. A
# payload the body allocated carries that second reference itself; a payload it
# merely BORROWS — a capture, a parameter, a module-level binding — carries none,
# so without one the discharge releases the delivery reference the resumer already
# consumed and the value dies under every holder that outlives the fiber.
#
# A first argument the compiler cannot read as a keyword set falls through to the
# `emit` primitive (docs/signals/emit.md § "Dynamic emit"), so the park is an
# ordinary call rather than the `Emit` terminator. Two positions, two references:
# a TAIL emit's payload already carries the borrowed-argument retain the call mints
# for the callee, which the post-`TailCall` block releases on resume, so the
# suspending exit must leave it standing (docs/impl/region/mechanism.md § "What the
# fall-through owes, a signal exit owes too"); a NON-TAIL emit has no such retain,
# so the site mints one of its own.
#
# The fault needs both ingredients, and the controls at the bottom remove one each:
# resume the fiber to completion, emit something the body allocated, or emit
# through the literal path, and the same program is balanced without an extra
# reference. So a fix that mints unconditionally trades this fault for a per-park
# leak, which the growth gauge at the end refuses.
#
# Each witness reads its subject AFTER the abandoned fiber's region is gone —
# through the borrow's own holder, through `fiber/value`, and through a container —
# so an over-early free faults rather than reading stale but mapped bytes. A fresh
# subject per iteration keeps region ids churning, so a recycled id detonates on its
# generation stamp.

(def sig :yield)

# ── (a) STATEMENT position, a module-level borrow ────────────────────────────
# The emit is a non-tail suspending call; nothing in the body releases `shared`.
(def shared (string "shared-subject"))
(defn w-module []
  (let [inner (fiber/new (fn ()
                           (emit sig shared)
                           0) |:yield|)]
    (length (fiber/resume inner))))

# ── (b) TAIL position, a module-level borrow ─────────────────────────────────
# Here the payload is a borrowed tail argument, so the call already mints the
# reference — the suspending exit must not consume it.
(defn w-module-tail []
  (let [inner (fiber/new (fn () (emit sig shared)) |:yield|)]
    (length (fiber/resume inner))))

# ── (c) STATEMENT position, a captured `let`-local ───────────────────────────
(defn w-local [n]
  (let [s (string "local" n)]
    (let [inner (fiber/new (fn ()
                             (emit sig s)
                             0) |:yield|)]
      (let [m (length (fiber/resume inner))]
        (%add m (length s))))))

# ── (d) TAIL position, a captured PARAMETER of the enclosing frame ───────────
(defn w-param-tail [s]
  (let [inner (fiber/new (fn () (emit sig s)) |:yield|)]
    (let [n (length (fiber/resume inner))]
      (%add n (length s)))))

# ── (e) the payload is read through `fiber/value`, not the resume ────────────
(defn w-fiber-value [s]
  (let [inner (fiber/new (fn ()
                           (emit sig s)
                           0) |:yield|)]
    (fiber/resume inner)
    (let [n (length (fiber/value inner))]
      (%add n (length s)))))

# ── (f) the borrow outlives the emitting frame in a container ────────────────
# Nothing in the frame reads `s` after the resume; the holder that must still find
# it alive is the array the caller reads back out.
(def @sink @[])
(defn w-stored [s]
  (push sink s)
  (let [inner (fiber/new (fn ()
                           (emit sig s)
                           0) |:yield|)]
    (length (fiber/resume inner))))

# ── (g) two dynamic parks of the same borrow, the second one abandoned ───────
(defn w-twice [s]
  (let [inner (fiber/new (fn ()
                           (emit sig s)
                           (emit sig s)
                           0) |:yield|)]
    (fiber/resume inner)
    (let [n (length (fiber/resume inner))]
      (%add n (length s)))))

# ── controls — remove one ingredient each; correct without an extra reference ─

# (h) the LITERAL path, which the `Emit` terminator already mints for.
(defn c-literal [s]
  (let [inner (fiber/new (fn ()
                           (emit :yield s)
                           0) |:yield|)]
    (let [n (length (fiber/resume inner))]
      (%add n (length s)))))

# (i) the literal path in TAIL position, the pair-control for (b) and (d).
(defn c-literal-tail [s]
  (let [inner (fiber/new (fn () (emit :yield s)) |:yield|)]
    (let [n (length (fiber/resume inner))]
      (%add n (length s)))))

# (j) the body ALLOCATES what it emits, so it owns the second reference.
(defn c-allocated [s]
  (let [inner (fiber/new (fn ()
                           (emit sig (concat s "!"))
                           0) |:yield|)]
    (let [n (length (fiber/resume inner))]
      (%add n (length s)))))

# (k) the body emits only a READ of the borrow — an immediate crosses no region.
(defn c-read [s]
  (let [inner (fiber/new (fn ()
                           (emit sig (length s))
                           0) |:yield|)]
    (fiber/resume inner)
    (length s)))

# (l) the fiber is RESUMED TO COMPLETION, so the body's continuation runs.
(defn c-completed [s]
  (let [inner (fiber/new (fn ()
                           (emit sig s)
                           0) |:yield|)]
    (let [n (length (fiber/resume inner))]
      (fiber/resume inner)
      (%add n (length s)))))

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
  (var m 0)
  (var n 0)
  (while (%lt i reps)
    (assign a (w-module))
    (assign b (w-module-tail))
    (assign c (w-local i))
    (assign d (w-param-tail (string "param" i)))
    (assign e (w-fiber-value (string "value" i)))
    (assign f (w-stored (string "stored" i)))
    (assign g (w-twice (string "twice" i)))
    (assign h (c-literal (string "lit" i)))
    (assign k (c-literal-tail (string "littail" i)))
    (assign l (c-allocated (string "alloc" i)))
    (assign m (c-read (string "read" i)))
    (assign n (c-completed (string "done" i)))
    # The witness (f) sink is a module-level container by design; read the stored
    # borrow back out — it must still be alive — then drain so the driver's own
    # retention stays flat.
    (assert (%gt (length (get sink (%sub (length sink) 1))) 0)
            "stored borrow freed by the abandoned fiber's discharge")
    (assign sink @[])
    (assign i (%add i 1)))
  (list a b c d e f g h k l m n))

(let [r (drive 800)]
  (assert (> (get r 7) 0) "control: literal emit mis-read (harness broken)")
  (assert (> (get r 8) 0) "control: literal tail emit mis-read (harness broken)")
  (assert (> (get r 9) 0) "control: body-allocated payload mis-read")
  (assert (> (get r 10) 0) "control: immediate payload mis-read")
  (assert (> (get r 11) 0) "control: completed fiber mis-read")
  (assert (> (get r 0) 0)
          "dynamic emit: module-level borrow freed by an abandoned park")
  (assert (> (get r 1) 0)
          "dynamic tail emit: module-level borrow freed by an abandoned park")
  (assert (> (get r 2) 0)
          "dynamic emit: captured local freed under the emitting frame")
  (assert (> (get r 3) 0)
          "dynamic tail emit: captured parameter freed under the caller")
  (assert (> (get r 4) 0)
          "dynamic emit: borrow freed under a `fiber/value` read")
  (assert (> (get r 5) 0)
          "dynamic emit: stored borrow freed under the container read")
  (assert (> (get r 6) 0)
          "dynamic emit: borrow freed by the second of two parks"))

# The module-level subject must survive every abandoned fiber that emitted it.
(assert (%gt (length shared) 0)
        "module-level borrow freed by an abandoned dynamic park")

# ── the leak face: one reference per park, not two ───────────────────────────
# The mint belongs only where the body owns none — and in tail position the
# borrowed-argument retain is already it — so steady-state region growth stays flat
# across the whole witness set.
(drive 100)
(let [before (arena/region-count)]
  (drive 400)
  (let [growth (%sub (arena/region-count) before)]
    (assert (%lt growth 40)
            (string "dynamic-emit borrow accounting strands regions: live count "
                    "grew by " growth " over 400 iterations of thirteen parks "
                    "each (expected flat)"))))

(println "region-dynamic-emit-borrow-uaf: ok")
