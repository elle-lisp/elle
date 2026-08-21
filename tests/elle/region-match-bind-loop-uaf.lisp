(elle/epoch 12)
# Soundness complement of region-match-bind-loop.lisp: placing a match
# scrutinee's release inside the loop that allocates it must not free anything
# early.
#
# Recording a `match` arm's pattern scope (docs/impl/region/mechanism.md § "Every
# binder records its scope") takes the scrutinee's release OUT of the hoisted
# position after the loop and back into the loop body, where it fires once per
# iteration. That is the direction that can over-free: a projection bound by the
# pattern lives INSIDE the scrutinee, so anything the arm hands that projection to
# is reading the scrutinee's pages after the release runs.
#
# Every witness below is such a hand-off, and each names the counted edge that has
# to keep the region standing — a cell's store, a container's store, a closure
# env's capture-incref, a fiber's park retain, a break's transfer to the enclosing
# block. If the per-iteration release drops the region to zero anyway, the read
# after the loop faults; a fresh scrutinee per iteration keeps region ids churning,
# so a freed region is recycled under the reader rather than read as stale but
# still-mapped bytes.

(def rounds 400)

# ── witnesses: a pattern-bound projection outlives the iteration that bound it ──

# (a) the arm stores the projection into a fn-local cell declared OUTSIDE the
# loop. The cell's counted store is what the per-iteration release must leave
# standing; the read happens after the loop, when every scrutinee but one has
# already been released.
(defn w-cell (n)
  (var out nil)
  (var i 0)
  (while (< i n)
    (match {:type :a :v (string "cell-" i)}
      {:type :a :v v} (assign out v)
      _ nil)
    (assign i (%add i 1)))
  (length out))

# (b) the arm stores the projection into a MODULE-level container, which outlives
# the frame entirely. The store funnel's incref is the surviving edge.
(def @sink @[])
(defn w-store (n)
  (var i 0)
  (while (< i n)
    (match {:type :a :v (string "store-" i)}
      {:type :a :v v} (push sink v)
      _ nil)
    (assign i (%add i 1)))
  (length (get sink (%sub (length sink) 1))))

# (c) the arm captures the projection in a CLOSURE that outlives the iteration.
# The env's capture-incref is the surviving edge, and the closure is called after
# the loop has released every scrutinee it allocated.
(defn w-capture (n)
  (var f nil)
  (var i 0)
  (while (< i n)
    (match {:type :a :v (string "cap-" i)}
      {:type :a :v v}
        (assign f (fn () (length v)))
      _ nil)
    (assign i (%add i 1)))
  (f))

# (d) the arm BREAKS out of the loop carrying the projection. The break transfers
# the value to the enclosing block, so its release belongs where the block's value
# is consumed — not to the iteration that bound it.
(defn w-break (n)
  (var i 0)
  (let [r (block (forever
                   (assign i (%add i 1))
                   (match {:type :a :v (string "brk-" i)}
                     {:type :a :v v} (when (< n i) (break v))
                     _ nil)))]
    (length r)))

# (e) the arm YIELDS the projection across the fiber frontier. The consumer reads
# it in another activation, after the yielding frame has parked mid-loop.
(defn w-yield (n)
  (let [fib (fiber/new (fn ()
                         (var i 0)
                         (while (< i n)
                           (match {:type :a :v (string "yld-" i)}
                             {:type :a :v v} (yield v)
                             _ nil)
                           (assign i (%add i 1)))
                         nil) |:yield|)]
    (var total 0)
    (while (not= (fiber/status fib) :dead)
      (let [s (fiber/resume fib)]
        (when (string? s)
          (assign total (%add total (length s))))))
    total))

# (f) the match sits in an INNER loop and the projection is read in the OUTER
# body, after the inner loop has run its releases.
(defn w-nested (n)
  (var acc 0)
  (var i 0)
  (while (< i n)
    (var keep nil)
    (var k 0)
    (while (< k 4)
      (match {:type :a :v (string "nst-" i "-" k)}
        {:type :a :v v} (assign keep v)
        _ nil)
      (assign k (%add k 1)))
    (assign acc (%add acc (length keep)))
    (assign i (%add i 1)))
  acc)

# (g) the projection is a whole nested CONTAINER, not a leaf: the read that must
# survive is a read into pages the scrutinee's free cascade would have reclaimed.
(defn w-nested-value (n)
  (var out nil)
  (var i 0)
  (while (< i n)
    (match {:type :a :v [(string "inner-" i) i]}
      {:type :a :v v} (assign out v)
      _ nil)
    (assign i (%add i 1)))
  (length (get out 0)))

# (h) the projection feeds the NEXT iteration's scrutinee — the value-succession
# shape, where the release of iteration k's scrutinee runs while iteration k+1
# still names the projection out of it.
(defn w-succession (n)
  (var s "seed")
  (var i 0)
  (while (< i n)
    (match {:type :a :v s :n i}
      {:type :a :v v}
        (assign s (string "sc-" (length v)))
      _ nil)
    (assign i (%add i 1)))
  (length s))

# ── control: the projection read only within the iteration that bound it ───────
(defn c-local (n)
  (var acc 0)
  (var i 0)
  (while (< i n)
    (match {:type :a :v (string "ctl-" i)}
      {:type :a :v v}
        (assign acc (%add acc (length v)))
      _ nil)
    (assign i (%add i 1)))
  acc)

(def cell-r (w-cell rounds))
(def store-r (w-store rounds))
(def capture-r (w-capture rounds))
(def break-r (w-break rounds))
(def yield-r (w-yield rounds))
(def nested-r (w-nested rounds))
(def nested-value-r (w-nested-value rounds))
(def succession-r (w-succession rounds))
(def local-r (c-local rounds))

(assert (> local-r 0) "control: in-arm read mis-read (harness broken)")

(assert (> cell-r 0) "projection freed under a cell that still holds it")
(assert (> store-r 0) "projection freed after being stored into a container")
(assert (> capture-r 0) "projection freed under the closure that captured it")
(assert (> break-r 0) "projection freed under the block the break handed it to")
(assert (> yield-r 0) "projection freed under the consumer of the yield")
(assert (> nested-r 0) "inner-loop projection freed under the outer body's read")
(assert (> nested-value-r 0)
        "nested container projection freed under a read into it")
(assert (> succession-r 0)
        "projection freed under the next iteration that names it")

(println "region-match-bind-loop-uaf: ok")
