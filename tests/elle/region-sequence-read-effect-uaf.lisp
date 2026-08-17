(elle/epoch 12)
# Soundness complement of region-sequence-read-effect.lisp: declaring the
# sequence reads `Opaque` must not free anything early.
#
# The declaration says two things and no more: the result may live anywhere (so
# the walk keeps recording `result ⊒ each argument`, and a borrowing read keeps
# its tighter `(alias, container)` record), and no argument is stored uncounted
# (so escape seeds nothing on its store facet). What that withdraws is the false
# store facet, and with it the refusal it forced on every mechanism gated on
# `sole_frame_held_regions` — the branch-arm release window among them
# (docs/impl/region/effects.md § `Opaque`; docs/impl/escape.md).
#
# So the hazards are the ones the withdrawn refusal used to mask. A read hands
# back a value living INSIDE its argument, so the argument must outlive the
# reader; a genuine escape of either the argument or the read's result must still
# refuse the window; and a value read out of a container and handed across a
# frontier must still be held up by the borrow accounting rather than by the seed.
# Each witness reads HEAP contents after the branch, through a chain long enough
# that an over-early free faults rather than reading stale but mapped bytes. A
# fresh subject per iteration keeps region ids churning.

# (a) the read's result is consumed AFTER the branch that produced it: the
# element still lives inside `v`, so `v` must outlive this read.
(defn w-read-after (v t)
  (let [r (match t
            :a (first v)
            :b (second v)
            _ (rest v))]
    (length r)))

# (b) the subject is read again after the branch. The window now moves `v`'s
# release to the merge, which must still land behind this second read.
(defn w-subject-after (v t)
  (match t
    :a (first v)
    :b (length v)
    _ (length v))
  (length (first v)))

# (c) the read's result ESCAPES — it is returned, and the caller reads it. The
# element lives inside `v`, so nothing may free `v` before the caller is done.
(defn w-escaped-read (v t)
  (match t
    :a (first v)
    :b (first v)
    _ (first v)))
(defn w-escaped-read-outer (v t)
  (length (w-escaped-read v t)))

# (d) the subject is STORED into a container outliving the frame by one arm while
# another arm reads it. The store is a real escape facet, so the window still
# refuses and the read back out must find the subject alive.
(def @sink @[])
(defn w-stored (v t)
  (match t
    :a (push sink v)
    :b (first v)
    _ 0)
  (length (get sink (%sub (length sink) 1))))

# (e) the read's result crosses a FIBER boundary — yielded out of the fiber, so
# the resumer reads an element that lives inside `v`.
(defn w-yielded (v)
  (let [f (fiber/new (fn ()
                       (yield (first v))
                       (yield (rest v))
                       0) |:yield|)]
    (fiber/resume f)
    (let [a (fiber/value f)]
      (fiber/resume f)
      (%add (length a) (length (fiber/value f))))))

# (f) the conversions: `->array`/`->list` may return arg0 itself, so their result
# and their argument can be the same value. Consuming the result after the branch
# must not free it through either name.
(defn w-converted (v t)
  (let [r (match t
            :a (->array v)
            :b (->list v)
            _ v)]
    (%add (length r) (length v))))

# ── controls: the same reads with no branch — correct now ────────────────────
(defn c-plain (v)
  (length (first v)))

# ── drive: fresh subject each iteration; an over-early free faults on the read ─
(var i 0)
(var a 0)
(var b 0)
(var c 0)
(var d 0)
(var e 0)
(var f 0)
(var g 0)
(while (%lt i 3000)
  (assign a (w-read-after (list (string "a" i) (string "aa" i)) :a))
  (assign b (w-subject-after (list (string "b" i) i) :a))
  (assign c (w-escaped-read-outer (list (string "c" i) i) :a))
  (assign d (w-stored (list (string "d" i) i) :a))
  (assign e (w-yielded (list (string "e" i) i)))
  (assign f (w-converted (list (string "f" i) i) :a))
  (assign g (c-plain (list (string "g" i) i)))
  # The sink is a module-level container by design (witness d stores into it);
  # drain it so the driver's own retention stays flat.
  (assign sink @[])
  (assign i (%add i 1)))

(assert (%gt g 0) "control: single-arm read mis-read (harness broken)")

(assert (%gt a 0) "container freed under the read's result")
(assert (%gt b 0) "subject freed under a second read after the branch")
(assert (%gt c 0)
        "container freed under the caller's read of the returned element")
(assert (%gt d 0) "stored subject freed by a sibling arm's read")
(assert (%gt e 0)
        "container freed under the resumer's read of a yielded element")
(assert (%gt f 0) "converted result freed under the post-branch read")

(println "region-sequence-read-effect-uaf: ok")
