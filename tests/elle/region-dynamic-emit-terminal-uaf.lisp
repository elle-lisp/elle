(elle/epoch 12)
# A raised payload's DELIVERY reference is the one the catcher's read consumes,
# and what raises here is the emit OPERATION rather than the `Emit` node
# (docs/impl/region/owner.md § "What yields is the emit OPERATION, not the
# `Emit` node").
#
# A first argument the compiler cannot read as a keyword set falls through to the
# `emit` primitive (docs/signals/emit.md § "Dynamic emit"), so a raise in TAIL
# position is an ordinary native call that leaves by a signal. That exit consumes
# the call's borrowed-argument retains, because the block that would have consumed
# them is abandoned — so the delivery is minted at the exit and recorded, exactly as
# `handle_emit` mints and records it on the literal path
# (docs/impl/region/mechanism.md § "What the fall-through owes, a signal exit owes
# too"). The record is what lets the abandoned-frame walk and the parked frame's
# discharge reclaim the frame's OWN reference to a payload it allocated, so the two
# references stay one per consumer.
#
# Every witness reads its payload after the raise has left the fiber — through the
# resume result, through `fiber/value`, through the borrow's own holder, and through
# a container — so an over-free faults at the deref (guardfree) or trips the
# generation check rather than reading stale but mapped bytes. A fresh subject per
# iteration keeps region ids churning, so a recycled id detonates on its stamp.
#
# The trap the restart faces guard: an `:error` fiber is resumable, so the parked
# frame replays the post-`TailCall` block and reaches the very release the exit
# already ran. The exit's nil stamp is what makes that second arrival a no-op —
# leaving the retain standing instead of minting the delivery reads correct until a
# restart claims it twice.
#
# Run under `--trace=guardfree` by the subprocess pin
# `region_dynamic_emit_terminal_uaf` in tests/integration/elle_scripts.rs.

(def sig :error)

# ── (a) a module-level borrow, raised from tail position ─────────────────────
# Nothing in the body releases `shared`, so the frame's only reference to it is
# the borrowed-argument retain the tail call minted.
(def shared (string "shared-subject"))
(defn w-module-tail []
  (let [f (fiber/new (fn () (emit sig shared)) |:error|)]
    (length (fiber/resume f))))

# ── (b) a captured `let`-local ───────────────────────────────────────────────
(defn w-local-tail [n]
  (let [s (string "local" n)]
    (let [f (fiber/new (fn () (emit sig s)) |:error|)]
      (let [m (length (fiber/resume f))]
        (%add m (length s))))))

# ── (c) a captured PARAMETER of the enclosing frame ──────────────────────────
(defn w-param-tail [s]
  (let [f (fiber/new (fn () (emit sig s)) |:error|)]
    (let [n (length (fiber/resume f))]
      (%add n (length s)))))

# ── (d) the payload read through `fiber/value` ───────────────────────────────
(defn w-fiber-value [s]
  (let [f (fiber/new (fn () (emit sig s)) |:error|)]
    (fiber/resume f)
    (let [n (length (fiber/value f))]
      (%add n (length s)))))

# ── (e) the borrow outlives the fiber in a container ─────────────────────────
(def @sink @[])
(defn w-stored [s]
  (push sink s)
  (let [f (fiber/new (fn () (emit sig s)) |:error|)]
    (length (fiber/resume f))))

# ── (f) the raise the fiber's mask does NOT catch ────────────────────────────
# The signal propagates out of the fiber and the caller's `catch` binds the
# payload, so the delivery is consumed one frontier further out.
(defn w-propagated [s]
  (let [f (fiber/new (fn () (emit sig s)) 0)]
    (try
      (fiber/resume f)
      (catch e (%add (length e) (length s))))))

# ── (g) a body-ALLOCATED payload, then a RESTART ─────────────────────────────
# Two references, two consumers: the delivery the caller's read consumes, and the
# frame's own, which the replayed block releases.
(defn w-owned-restart [n]
  (let [f (fiber/new (fn () (emit sig (string "owned" n))) |:error|)]
    (let [v (fiber/resume f)]
      (let [m (length v)]
        (try
          (fiber/resume f)
          (catch e nil))
        (%add m (length v))))))

# ── (h) a BORROWED payload, then a restart ───────────────────────────────────
(defn w-borrow-restart [s]
  (let [f (fiber/new (fn () (emit sig s)) |:error|)]
    (let [v (fiber/resume f)]
      (let [m (length v)]
        (try
          (fiber/resume f)
          (catch e nil))
        (%add m (length s))))))

# ── controls — remove one ingredient each; correct with no delivery mint ─────

# (i) the LITERAL path, which the `Emit` terminator already mints for.
(defn c-literal-tail [s]
  (let [f (fiber/new (fn () (emit :error s)) |:error|)]
    (let [n (length (fiber/resume f))]
      (%add n (length s)))))

# (j) STATEMENT position: the raise leaves a call whose own site mints the
# reference the park owes, and the exit under test is never reached.
(defn c-stmt [s]
  (let [f (fiber/new (fn ()
                       (emit sig s)
                       9) |:error|)]
    (let [n (length (fiber/resume f))]
      (%add n (length s)))))

# (k) an immediate payload crosses no region at all.
(defn c-immediate [s]
  (let [f (fiber/new (fn () (emit sig (length s))) |:error|)]
    (fiber/resume f)
    (length s)))

# (l) a body-allocated payload with no restart: the frame's own reference is the
# only one the walk reclaims, and the read must still find the value.
(defn c-allocated [n]
  (let [f (fiber/new (fn () (emit sig (string "alloc" n))) |:error|)]
    (length (fiber/resume f))))

# (m) one region named through BOTH arguments: the first occurrence moves the
# frame's reference and the second takes a retain of its own, so the payload is
# delivered once out of two names for it.
(defn c-repeat []
  (let [f (fiber/new (fn ()
                       (let [t (set :error)]
                         (emit t t))) |:error|)]
    (length (fiber/resume f))))

# (n) the SUSPENDING half of the same retain: a park, whose reference the
# replayed block releases rather than the catcher.
(defn c-yield-tail [s]
  (let [f (fiber/new (fn () (emit :yield s)) |:yield|)]
    (let [n (length (fiber/resume f))]
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
  (var o 0)
  (var p 0)
  (while (%lt i reps)
    (assign a (w-module-tail))
    (assign b (w-local-tail i))
    (assign c (w-param-tail (string "param" i)))
    (assign d (w-fiber-value (string "value" i)))
    (assign e (w-stored (string "stored" i)))
    (assign f (w-propagated (string "prop" i)))
    (assign g (w-owned-restart i))
    (assign h (w-borrow-restart (string "restart" i)))
    (assign k (c-literal-tail (string "lit" i)))
    (assign l (c-stmt (string "stmt" i)))
    (assign m (c-immediate (string "imm" i)))
    (assign n (c-allocated i))
    (assign o (c-repeat))
    (assign p (c-yield-tail (string "yield" i)))
    # The (e) sink is a module-level container by design: read the stored borrow
    # back out — it must still be alive — then drain, so the driver's own
    # retention stays flat.
    (assert (%gt (length (get sink (%sub (length sink) 1))) 0)
            "stored borrow freed by the raising fiber")
    (assign sink @[])
    (assign i (%add i 1)))
  (list a b c d e f g h k l m n o p))

(let [r (drive 400)]
  (assert (> (get r 8) 0)
          "control: literal tail raise mis-read (harness broken)")
  (assert (> (get r 9) 0) "control: statement-position raise mis-read")
  (assert (> (get r 10) 0) "control: immediate payload mis-read")
  (assert (> (get r 11) 0) "control: body-allocated payload mis-read")
  (assert (> (get r 12) 0) "control: two-name payload mis-read")
  (assert (> (get r 13) 0) "control: suspending tail emit mis-read")
  (assert (> (get r 0) 0)
          "dynamic tail raise: module-level borrow freed under the caller's read")
  (assert (> (get r 1) 0)
          "dynamic tail raise: captured local freed under the emitting frame")
  (assert (> (get r 2) 0)
          "dynamic tail raise: captured parameter freed under the caller")
  (assert (> (get r 3) 0)
          "dynamic tail raise: payload freed under a `fiber/value` read")
  (assert (> (get r 4) 0)
          "dynamic tail raise: stored borrow freed under the container read")
  (assert (> (get r 5) 0)
          "dynamic tail raise: propagated payload freed under the catcher")
  (assert (> (get r 6) 0)
          "dynamic tail raise: allocated payload freed by the restart's replay")
  (assert (> (get r 7) 0)
          "dynamic tail raise: borrowed payload freed by the restart's replay"))

# The module-level subject must survive every fiber that raised it.
(assert (%gt (length shared) 0)
        "module-level borrow freed by a dynamic tail raise")

# ── the leak face: one delivery per raise, not one per reference ─────────────
# The mint answers to the catcher's single release, and the frame's own reference
# to the frames' own release tables, so steady-state region growth stays flat
# across the whole witness set.
(drive 100)
(let [before (arena/region-count)]
  (drive 400)
  (let [growth (%sub (arena/region-count) before)]
    (assert (%lt growth 40)
            (string "terminal dynamic-emit delivery strands regions: live count "
                    "grew by " growth " over 400 iterations of fourteen raises "
                    "each (expected flat)"))))

(println "region-dynamic-emit-terminal-uaf: ok")
