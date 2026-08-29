(elle/epoch 12)
# A raised payload's DELIVERY reference is the one the catcher's read consumes, and
# the site's own retain answers to the continuation past the call — two references,
# two consumers, wherever the raise leaves the emit PRIMITIVE (docs/impl/region/owner.md
# § "What yields is the emit OPERATION, not the `Emit` node").
#
# A first argument the compiler cannot read as a keyword set falls through to that
# primitive (docs/signals/emit.md § "Dynamic emit"), so a raise OFF TAIL POSITION is an
# ordinary native call whose site mints one retain at the payload argument and releases
# it in the continuation. The signal is a runtime value, so that retain is taken
# whatever the signal turns out to be — and a terminal `:error` adds the catcher beside
# the continuation. An `:error` fiber is resumable, so both consumers run: the catcher
# at the raise, the continuation at a restart's replay. The raise therefore mints the
# delivery itself at the signal exit and records it, and the site's retain is left to
# the continuation.
#
# The trap: with a single `fiber/resume` the shape reads correct either way. The parked
# frame never replays, so the site's retain reaches no consumer but the catcher and
# stands in for the delivery. Only a RESTART brings the second consumer, which is why
# every witness here resumes twice and control (o) — the same body resumed once —
# stays clean.
#
# The counter-factual the record answers for: a fiber nobody restarts reaches the
# continuation only through the frames' own release table, so the site's stash must be
# a recorded value route (docs/impl/region/mechanism.md § "An abandoned frame runs the
# releases it still owes"). Mint the delivery without recording the route and the same
# programs strand one region per raise instead — the growth gate at the bottom refuses
# that trade.
#
# Every witness reads its payload after the raise has left the fiber — through the
# resume result, through `fiber/value`, through the borrow's own holder, and through a
# container — so an over-free faults at the deref rather than reading stale but mapped
# bytes. A fresh subject per iteration keeps region ids churning, so a recycled id
# detonates on its generation stamp.
#
# Run under `--trace=guardfree` by the subprocess pin
# `region_dynamic_emit_statement_uaf` in tests/integration/elle_scripts.rs.

(def sig :error)

# ── (a) a module-level borrow, raised in statement position, then RESTARTED ──
# Nothing in the body releases `shared`, so the frame's only reference to it is the
# retain the site minted — and the replayed continuation is what releases that.
(def shared (string "shared-subject"))
(defn w-module [i]
  (let [f (fiber/new (fn ()
                       (emit sig shared)
                       9) |:error|)]
    (let [v (fiber/resume f)]
      (let [n (length v)]
        (try
          (fiber/resume f)
          (catch e nil))
        (%add n (length shared))))))

# ── (b) a captured `let`-local ───────────────────────────────────────────────
(defn w-local [i]
  (let [s (string "local" i)]
    (let [f (fiber/new (fn ()
                         (emit sig s)
                         9) |:error|)]
      (let [v (fiber/resume f)]
        (let [n (length v)]
          (try
            (fiber/resume f)
            (catch e nil))
          (%add n (length s)))))))

# ── (c) a captured PARAMETER of the enclosing frame ──────────────────────────
(defn w-param [s]
  (let [f (fiber/new (fn ()
                       (emit sig s)
                       9) |:error|)]
    (let [v (fiber/resume f)]
      (let [n (length v)]
        (try
          (fiber/resume f)
          (catch e nil))
        (%add n (length s))))))

# ── (d) a body-ALLOCATED payload ─────────────────────────────────────────────
# The body owns a reference of its own here, so the site mints none — what the
# restart's replay releases is the body's, and the catcher's is the delivery.
(defn w-owned [i]
  (let [f (fiber/new (fn ()
                       (emit sig (string "owned" i))
                       9) |:error|)]
    (let [v (fiber/resume f)]
      (let [n (length v)]
        (try
          (fiber/resume f)
          (catch e nil))
        (%add n (length v))))))

# ── (e) the payload read through `fiber/value` after the restart ─────────────
(defn w-fiber-value [s]
  (let [f (fiber/new (fn ()
                       (emit sig s)
                       9) |:error|)]
    (fiber/resume f)
    (let [n (length (fiber/value f))]
      (try
        (fiber/resume f)
        (catch e nil))
      (%add n (length s)))))

# ── (f) the borrow outlives the fiber in a container ─────────────────────────
(def @sink @[])
(defn w-stored [s]
  (push sink s)
  (let [f (fiber/new (fn ()
                       (emit sig s)
                       9) |:error|)]
    (let [v (fiber/resume f)]
      (let [n (length v)]
        (try
          (fiber/resume f)
          (catch e nil))
        n))))

# ── (g) the raise the fiber's mask does NOT catch, then a restart ────────────
# The signal propagates out of the fiber and the caller's `catch` binds the
# payload, so the delivery is consumed one frontier further out.
(defn w-propagated [s]
  (let [f (fiber/new (fn ()
                       (emit sig s)
                       9) 0)]
    (let [n (try
              (fiber/resume f)
              (catch e (length e)))]
      (try
        (fiber/resume f)
        (catch e nil))
      (+ n (length s)))))

# ── (h) TWO restarts of one raise ────────────────────────────────────────────
# The replay runs the continuation's release once; the third resume finds the
# fiber dead and must not reach it a second time.
(defn w-twice [s]
  (let [f (fiber/new (fn ()
                       (emit sig s)
                       9) |:error|)]
    (let [v (fiber/resume f)]
      (let [n (length v)]
        (try
          (fiber/resume f)
          (catch e nil))
        (try
          (fiber/resume f)
          (catch e nil))
        (%add n (length s))))))

# ── (i) one region named through BOTH arguments ──────────────────────────────
# The signal and the payload are one value, so the identity gate matches on the
# payload and the delivery is minted once out of two names for it.
(defn w-repeat []
  (let [f (fiber/new (fn ()
                       (let [t (set :error)]
                         (emit t t)
                         9)) |:error|)]
    (let [v (fiber/resume f)]
      (let [n (length v)]
        (try
          (fiber/resume f)
          (catch e nil))
        n))))

# ── controls — remove one ingredient each; correct with no delivery mint ─────

# (j) the LITERAL path, whose `Emit` terminator mints no body reference for a
# terminal signal at all, so its replayed continuation releases nothing.
(defn c-literal [s]
  (let [f (fiber/new (fn ()
                       (emit :error s)
                       9) |:error|)]
    (let [v (fiber/resume f)]
      (let [n (length v)]
        (try
          (fiber/resume f)
          (catch e nil))
        (%add n (length s))))))

# (k) TAIL position: the exit consumes the borrowed-argument retain itself and
# nil-stamps its stash, so the replay finds an immediate there.
(defn c-tail [s]
  (let [f (fiber/new (fn () (emit sig s)) |:error|)]
    (let [v (fiber/resume f)]
      (let [n (length v)]
        (try
          (fiber/resume f)
          (catch e nil))
        (%add n (length s))))))

# (l) an immediate payload crosses no region at all.
(defn c-immediate [s]
  (let [f (fiber/new (fn ()
                       (emit sig (length s))
                       9) |:error|)]
    (fiber/resume f)
    (try
      (fiber/resume f)
      (catch e nil))
    (length s)))

# (m) the SUSPENDING twin: the site's retain is the body reference the park owes,
# and the resume replays the very continuation that releases it — one consumer.
(defn c-yield [s]
  (let [f (fiber/new (fn ()
                       (emit :yield s)
                       9) |:yield|)]
    (let [v (fiber/resume f)]
      (let [n (length v)]
        (fiber/resume f)
        (%add n (length s))))))

# (n) the DYNAMIC suspending twin, where the site mints the same retain but the
# signal resolves to a park rather than a raise.
(defn c-yield-dyn [s]
  (let [ysig :yield]
    (let [f (fiber/new (fn ()
                         (emit ysig s)
                         9) |:yield|)]
      (let [v (fiber/resume f)]
        (let [n (length v)]
          (fiber/resume f)
          (%add n (length s)))))))

# (o) NO restart: the parked frame never replays, so the site's retain reaches
# only the catcher. This is the shape that reads correct either way.
(defn c-once [s]
  (let [f (fiber/new (fn ()
                       (emit sig s)
                       9) |:error|)]
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
  (var q 0)
  (while (%lt i reps)
    (assign a (w-module i))
    (assign b (w-local i))
    (assign c (w-param (string "param" i)))
    (assign d (w-owned i))
    (assign e (w-fiber-value (string "value" i)))
    (assign f (w-stored (string "stored" i)))
    (assign g (w-propagated (string "prop" i)))
    (assign h (w-twice (string "twice" i)))
    (assign k (w-repeat))
    (assign l (c-literal (string "lit" i)))
    (assign m (c-tail (string "tail" i)))
    (assign n (c-immediate (string "imm" i)))
    (assign o (c-yield (string "yield" i)))
    (assign p (c-yield-dyn (string "ydyn" i)))
    (assign q (c-once (string "once" i)))
    # The (f) sink is a module-level container by design: read the stored borrow
    # back out — it must still be alive — then drain, so the driver's own
    # retention stays flat.
    (assert (%gt (length (get sink (%sub (length sink) 1))) 0)
            "stored borrow freed by the restarted fiber's replay")
    (assign sink @[])
    (assign i (%add i 1)))
  (list a b c d e f g h k l m n o p q))

(let [r (drive 400)]
  (assert (> (get r 9) 0)
          "control: literal statement raise mis-read (harness broken)")
  (assert (> (get r 10) 0) "control: tail raise mis-read")
  (assert (> (get r 11) 0) "control: immediate payload mis-read")
  (assert (> (get r 12) 0) "control: literal suspending emit mis-read")
  (assert (> (get r 13) 0) "control: dynamic suspending emit mis-read")
  (assert (> (get r 14) 0) "control: unrestarted raise mis-read")
  (assert (> (get r 0) 0)
          "statement raise: module-level borrow freed by the restart's replay")
  (assert (> (get r 1) 0)
          "statement raise: captured local freed by the restart's replay")
  (assert (> (get r 2) 0)
          "statement raise: captured parameter freed by the restart's replay")
  (assert (> (get r 3) 0)
          "statement raise: body-allocated payload freed by the restart's replay")
  (assert (> (get r 4) 0)
          "statement raise: payload freed under a `fiber/value` read")
  (assert (> (get r 5) 0)
          "statement raise: stored borrow freed under the container read")
  (assert (> (get r 6) 0)
          "statement raise: propagated payload freed under the catcher")
  (assert (> (get r 7) 0) "statement raise: payload freed by the second restart")
  (assert (> (get r 8) 0)
          "statement raise: two-name payload freed by the restart's replay"))

# The module-level subject must survive every fiber that raised it.
(assert (%gt (length shared) 0)
        "module-level borrow freed by a restarted statement-position raise")

# ── the leak face: one delivery per raise, and one consumer per retain ───────
# The mint answers to the catcher's single release and the site's retain to the
# continuation — which a fiber nobody restarts reaches only through the frames'
# own release table. A mint with no such route strands one region per raise.
(drive 100)
(let [before (arena/region-count)]
  (drive 400)
  (let [growth (%sub (arena/region-count) before)]
    (assert (%lt growth 40)
            (string "statement dynamic-emit delivery strands regions: live count "
                    "grew by " growth " over 400 iterations of fifteen raises "
                    "each (expected flat)"))))

(println "region-dynamic-emit-statement-uaf: ok")
