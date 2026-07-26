(elle/epoch 12)
# Soundness complement of region-break-skip.lisp: re-anchoring a release the
# `break` jumps over must not free anything early.
#
# The close moves a release from inside a block's body to `last_use[block]`
# (docs/impl/region/mechanism.md § "A release the break jumps over is not a
# release"). Moving a release LATER can only over-keep — but only while it still
# names the same value when it runs. Three ways that fails, and all of them fault
# here: the value-route reloads a slot a later store has repointed (freeing the
# live occupant), a region reached across a nested loop is released once for many
# allocations (freeing an iteration that is still live), and a region reached
# across a nested lambda is released from the wrong frame entirely.
#
# Every witness reads the subject's HEAP contents after the block, through a
# chain long enough that an over-early free faults rather than reading stale but
# still-mapped bytes. A fresh subject per iteration keeps region ids churning so
# a freed region is recycled under the reader.

# ── witnesses: a value in the skipped window survives its later use ───────────

# (a) the window value is the block's FALL-THROUGH value, read after the block.
# The break never fires here, so the value must survive the moved release.
(defn w-fallthrough (i)
  (let [r (block (let [x (list (string "p" i) (string "q" i))]
                   (when (%lt i 0) (break nil))
                   x))]
    (length (first r))))

# (b) the window value ESCAPES into a container that outlives the frame, and is
# read back out afterwards. The store's incref is what the moved release must
# balance against — releasing the producer's reference is correct, freeing the
# stored value is not.
(def @sink @[])
(defn w-store (i)
  (block (let [x (string "s" i)]
           (when (%lt i 0) (break nil))
           (push sink x)))
  (length (get sink (%sub (length sink) 1))))

# (c) the window value is RETURNED — the caller's read must see it alive, so the
# moved release must land after the return mint, not before it.
(defn w-return-inner (i)
  (block (let [x (list (string "r" i) i)]
           (when (%lt i 0) (break nil))
           x)))
(defn w-return (i)
  (length (first (w-return-inner i))))

# (d) the window value is CAPTURED by a closure the block hands back; the
# capture's incref must outlive the moved release.
(defn w-capture (i)
  (let [f (block (let [x (string "c" i)]
                   (when (%lt i 0) (break nil))
                   (fn () (length x))))]
    (f)))

# (e) a nested LOOP inside the window: each iteration allocates its own value and
# reads it. Releasing these at the block's exit instead would free one region for
# eight allocations — every earlier iteration's value already recycled under the
# next read.
(defn w-loop (i)
  (block (when (%lt i 0) (break nil))
    (def @k 0)
    (def @n 0)
    (while (%lt k 8)
      (let [x (list (string "l" i "-" k) k)]
        (assign n (%add n (length (first x)))))
      (assign k (%add k 1)))
    n))

# (f) a nested LAMBDA inside the window, called repeatedly: its body's releases
# belong to the closure's activation. Hoisting one into the enclosing block would
# release a region resolved against the wrong frame's slot.
(defn w-lambda (i)
  (block (when (%lt i 0) (break nil))
    (let [f (fn (j)
              (let [x (list (string "m" i "-" j) j)]
                (length (first x))))]
      (%add (f 1) (f 2)))))

# (g) the break DOES fire, and a window value escaped before it: the release the
# jump used to skip now runs, and it must drop only the producer's reference.
(defn w-break-taken (i)
  (block (let [x (string "t" i)]
           (push sink x)
           (when (%gt i -1) (break 1))
           (length x)))
  (length (get sink (%sub (length sink) 1))))

# (h) an OUTER reassigned binding written inside the window: its slot no longer
# names the window value by the block's exit, so the value route must stay off it
# (the mutated-slot backstop, docs/impl/region/bindings.md).
(defn w-reassign (i)
  (def @cur (string "u" i))
  (block (when (%lt i 0) (break nil))
    (assign cur (string "v" i "-long"))
    nil)
  (length cur))

# ── controls: the same reads with no break — correct now (harness sanity) ─────
(defn c-plain (i)
  (let [r (block (let [x (string "d" i)]
                   x))]
    (length r)))

# ── drive: fresh subject each iteration; an over-early free faults on the read ─
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
(while (%lt i 3000)
  (assign a (w-fallthrough i))
  (assign b (w-store i))
  (assign c (w-return i))
  (assign d (w-capture i))
  (assign e (w-loop i))
  (assign f (w-lambda i))
  (assign g (w-break-taken i))
  (assign h (w-reassign i))
  (assign k (c-plain i))
  # The sink is a module-level container by design (witnesses b and g store into
  # it); drain it so the driver's own retention stays flat.
  (assign sink @[])
  (assign i (%add i 1)))

(assert (%gt k 0) "control: plain block result mis-read (harness broken)")

(assert (%gt a 0)
        "window value that is the block's fall-through result freed under the \
         post-block read")
(assert (%gt b 0) "window value freed after being stored into a container")
(assert (%gt c 0) "window value freed under the caller's read of the return")
(assert (> d 0) "window value freed under the closure that captured it")
(assert (%gt e 0) "loop-body value freed under a later iteration's read")
(assert (%gt f 0) "lambda-body value released from the enclosing frame")
(assert (%gt g 0)
        "window value freed by the release the taken break used to skip")
(assert (%gt h 0) "reassigned binding's live value freed through its slot")

(println "region-break-skip-uaf: ok")
