(elle/epoch 12)
# A caught `fiber/abort` mints the reference its result is released against.
#
# Aborting a :paused fiber injects the caller's payload into the fiber and
# resumes it to unwind. Where the mask catches, that payload comes straight back
# out as the abort's RESULT, so the caller's `DecrefValueRegion` fires on it
# alongside the separate one the caller already owes it as an ARGUMENT — and the
# unwinding exit runs no `Return` to fund the second.
# `handle_fiber_abort_signal` mints it, the same argument `do_fiber_abort` makes
# for the value it hands to a replayed inner frame.
#
# Without the mint the payload's region is freed while the fiber (and the
# reader below) still points into it. Run under `--trace=guardfree`, where every
# freed page is PROT_NONE, each read below faults at the dereference.
#
# The mint is owed by the result, so only the arm that hands one back takes it —
# the placement face below pins that. `region-fiber-install-clique-leak.lisp` is
# the bounded-growth face of the `Delivers` declaration this belongs to.

# ── The caught face: the abort's result IS the injected payload ──────────
# The mask catches :error, so the abort's caller receives the payload back and
# releases it as a result on top of releasing it as an argument.
(let [f (fiber/new (fn []
                     (yield 1)
                     2) |:yield :error|)]
  (fiber/resume f)
  (let [r (fiber/abort f [1 2 3])]
    (assert (= r [1 2 3])
            "fiber/abort hands the injected payload back to its caller")
    (assert (= (get r 2) 3)
            "the payload's contents are readable after the abort")))

# The payload also stays readable through the fiber it was parked on.
(let [f (fiber/new (fn []
                     (yield 1)
                     2) |:yield :error|)]
  (fiber/resume f)
  (fiber/abort f [4 5 6])
  (assert (= (fiber/value f) [4 5 6])
          "the aborted fiber's terminal value is the injected payload"))

# ── The unwound face: defer/protect runs before the payload comes back ───
# The body's cleanup allocates and frees while the payload is in flight, so a
# freed payload region is recycled under it before the caller reads it.
(let [f (fiber/new (fn []
                     (defer
                       (length [1 2 3 4 5])
                       (yield 1)
                       2)) |:yield :error|)]
  (fiber/resume f)
  (let [r (fiber/abort f [7 8 9])]
    (assert (= (get r 0) 7)
            "the payload survives the aborted body's defer cleanup")))

# ── Placement: only the arm that hands a result back mints ───────────────
# The mint is owed by the RESULT, not by the delivery. An in-body handler that
# catches the injected error consumes the payload inside the fiber and hands the
# caller a value of its own, so no caller release targets the payload and a mint
# taken at the delivery would strand it once per abort.
(defn caught-churn [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (let [f (fiber/new (fn []
                         (protect (yield 1))
                         7) |:yield :error|)]
      (fiber/resume f)
      (fiber/abort f [1 2 3]))
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(let [d (caught-churn 200)]
  (assert (%lt d 30)
          (string "abort payload caught in body: 200 iters grew the region count by "
                  d " (only the result arm owes the missing Return's mint)")))

# ── Churn: repeated abort delivery must not accumulate stale pages ───────
(def @i 0)
(while (%lt i 200)
  (let [f (fiber/new (fn []
                       (yield 1)
                       2) |:yield :error|)]
    (fiber/resume f)
    (assert (= (get (fiber/abort f [1 2 3]) 1) 2)
            "repeated abort delivery keeps the payload live"))
  (assign i (%add i 1)))
