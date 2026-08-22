(elle/epoch 12)
# A propagated signal is a fresh park, and owes its own delivery reference
# (docs/impl/region/owner.md § "Park/unpark symmetry").
#
# `fiber/propagate` installs the child's parked payload as the propagating
# fiber's own `signal`. That fiber's resumer reads the payload as its resume
# result and runs the compiler-emitted release on it — the consumer an `Emit`
# funds with its `EmitEscape` mint. Re-parking a value the child already parked
# mints nothing, so the release consumes a reference the propagate never took.
#
# The trap: ONE propagate hides it. An error unwind runs no continuation, so the
# raising body's own reference is stranded and unclaimed, and the release eats
# that instead — which is why the obvious three-line reproducer is green. The
# witnesses below each remove that cover, by propagating twice or by raising the
# error from a native, whose payload reaches `fiber.signal` owning nothing.
#
# The counter-factual: without the mint the payload's count runs one short of
# the recorded `fiber → payload` edges, so the last fiber's free cascade
# reclaims it while the caller still holds it. Every witness reads a HEAP field
# of the payload after the carrying fibers are gone, never just `(not ok?)` —
# a bare status check passes over a freed payload and would have missed this.
#
# `defer` is the propagate in production form: it resumes a body fiber, runs
# cleanup, then `(fiber/propagate f)` when the body did not complete. `protect`
# supplies the outermost catch and hands the payload back as data.

# ── (a) two propagates: the second has no stranded reference to steal ────────
(defn a-inner [n]
  (defer
    nil
    (error {:reason :bang :tag (string "a" n)})))
(defn a-middle [n]
  (defer
    nil
    (a-inner n)))
(defn w-two-propagates [n]
  (let [[ok? err] (protect (a-middle n))]
    (assert (not ok?) "two-propagate witness completed instead of erroring")
    (assert (= err:reason :bang) "two-propagate payload lost its :reason")
    (length err:tag)))

# ── (b) three propagates — the shortfall is per park, not per value ──────────
(defn b-deep [n]
  (defer
    nil
    (defer
      nil
      (defer
        nil
        (error {:reason :deep :tag (string "b" n)})))))
(defn w-three-propagates [n]
  (let [[ok? err] (protect (b-deep n))]
    (assert (not ok?) "three-propagate witness completed instead of erroring")
    (length err:tag)))

# ── (c) a NATIVE error through one propagate ─────────────────────────────────
# A native raise builds its payload and installs it without leaving a body
# reference behind, so a single propagate is already one short.
(defn c-native [n]
  (defer
    nil
    (get nil :missing)))
(defn w-native [n]
  (let [[ok? err] (protect (c-native n))]
    (assert (not ok?) "native-error witness completed instead of erroring")
    (assert (= err:error :type-error) "native payload lost its :error tag")
    (length err:message)))

# ── (d) the payload read through `fiber/value`, not the protect tuple ────────
# The explicit form of the same park: propagate out of a hand-rolled fiber and
# read the result off the outer fiber after control has left it.
(defn d-body [n]
  (defer
    nil
    (error {:reason :bang :tag (string "d" n)})))
(defn w-fiber-value [n]
  (let [f (fiber/new (fn () (d-body n)) 1)]
    (fiber/resume f nil)
    (assert (not (= (fiber/status f) :dead))
            "fiber-value witness completed instead of erroring")
    (let [err (fiber/value f)]
      (length err:tag))))

# ── (e) the payload outlives the read, in a container ────────────────────────
# Nothing on the stack holds it after the protect returns; the holder that must
# still find it alive is the array the driver reads back out.
(def @errsink @[])
(defn w-stored [n]
  (let [[ok? err] (protect (a-middle n))]
    (push errsink err)
    (length err:tag)))

# ── controls — each removes one ingredient; balanced without a minted ref ────

# (f) ONE propagate of a body-allocated payload: the stranded raise reference
# covers the delivery, so this is green with or without the mint.
(defn g-one [n]
  (defer
    nil
    (error {:reason :bang :tag (string "f" n)})))
(defn c-one-propagate [n]
  (let [[ok? err] (protect (g-one n))]
    (length err:tag)))

# (g) the body COMPLETES, so `defer` never propagates at all.
(defn g-ok [n]
  (defer
    nil
    (string "g" n)))
(defn c-completed [n]
  (let [[ok? v] (protect (g-ok n))]
    (assert ok? "control: completing body reported an error")
    (length v)))

# (h) the payload is an IMMEDIATE — no region crosses, so nothing is delivered.
(defn h-imm [n]
  (defer
    nil
    (defer
      nil
      (error 42))))
(defn c-immediate [n]
  (let [[ok? err] (protect (h-imm n))]
    (assert (not ok?) "control: immediate-payload witness did not error")
    (if (= err 42) 1 0)))

# ── drive: a fresh payload per iteration keeps region ids churning, so a ─────
# recycled id detonates on its generation stamp rather than reading stale bytes.

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
  (while (%lt i reps)
    (assign a (w-two-propagates i))
    (assign b (w-three-propagates i))
    (assign c (w-native i))
    (assign d (w-fiber-value i))
    (assign e (w-stored i))
    (assign f (c-one-propagate i))
    (assign g (c-completed i))
    (assign h (c-immediate i))
    # Witness (e)'s holder is the module-level array by design: read the stored
    # payload back out — it must still be alive — then drain so the driver's
    # own retention stays flat for the growth gauge below.
    (let [held (get errsink (%sub (length errsink) 1))]
      (assert (%gt (length held:tag) 0)
              "stored payload freed by the propagating fibers' free cascade"))
    (assign errsink @[])
    (assign i (%add i 1)))
  (list a b c d e f g h))

(let [r (drive 400)]
  (assert (> (get r 0) 0) "payload freed by the second of two propagates")
  (assert (> (get r 1) 0) "payload freed by the third of three propagates")
  (assert (> (get r 2) 0) "native-error payload freed by its propagate")
  (assert (> (get r 3) 0) "payload freed under a `fiber/value` read")
  (assert (> (get r 4) 0) "payload freed under the container read")
  (assert (> (get r 5) 0) "control: single propagate mis-read (harness broken)")
  (assert (> (get r 6) 0) "control: completing body mis-read")
  (assert (> (get r 7) 0) "control: immediate payload mis-read"))

# ── the leak face: the mint must fund exactly one consumer, per park ─────────
# A mint no release answers strands one region per propagate, so the gauge is
# DIFFERENTIAL in propagate depth, not absolute: raise the same payload through
# zero, one, and three propagates and compare the growth of each.
#
# The trap: absolute flatness is not available to measure against. `(protect
# (error {...}))` already strands two regions per iteration with no propagate
# anywhere in it — the raising body's own reference to the payload it allocated,
# whose release lives in a continuation an error unwind never runs. That leak
# predates this mint and is identical with and without it. Asserting a flat
# region count here would pin that unrelated leak and fail forever.
#
# The counter-factual: an unconsumed mint makes growth scale with depth, so
# `d3` would exceed `d0` by three regions per iteration and the slack below
# would not absorb it.

(defn d0 [n]
  (let [[ok? err] (protect (error {:reason :bang :tag (string "z" n)}))]
    (length err:tag)))
(defn d1 [n]
  (let [[ok? err] (protect (defer
                             nil
                             (error {:reason :bang :tag (string "z" n)})))]
    (length err:tag)))
(defn d3 [n]
  (let [[ok? err] (protect (defer
                             nil
                             (defer
                               nil
                               (defer
                                 nil
                                 (error {:reason :bang :tag (string "z" n)})))))]
    (length err:tag)))

(defn growth-of [f reps]
  (var i 0)
  (while (%lt i 50)
    (f i)
    (assign i (%add i 1)))
  (let [before (arena/region-count)]
    (assign i 0)
    (while (%lt i reps)
      (f i)
      (assign i (%add i 1)))
    (%sub (arena/region-count) before)))

(let [g0 (growth-of d0 400)
      g1 (growth-of d1 400)
      g3 (growth-of d3 400)]
  (assert (%lt (%sub g1 g0) 40)
          (string "one propagate strands regions: growth " g1 " vs " g0
                  " with no propagate, over 400 iterations"))
  (assert (%lt (%sub g3 g0) 40)
          (string "each propagate strands a region: growth " g3
                  " at depth three vs " g0 " at depth zero, over 400 iterations")))

(println "region-fiber-propagate-uaf: ok")
