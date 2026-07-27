(elle/epoch 12)
# A fn-local reassigned mutable is a 1-slot container, and a container's
# reference to its content needs TWO release channels, not one
# (docs/impl/region/bindings.md § "Reassigned mutable bindings are 1-slot
# containers").
#
# The cell holds exactly one reference to whatever it currently points at. That
# reference dies at the OVERWRITE for every displaced prior, and at the cell's
# own DEMISE for the final, never-overwritten content. The producer's reference
# is a second, independent claim: it dies at the store, where the cell's
# reference takes over.
#
# Collapsing the three into fewer releases breaks in a different place each way.
# Reuse the producer's release as the cell's demise and a loop breaks it: one
# release, fired once, against whichever value the producer slot happens to hold
# last — every earlier value keeps a reference nobody drops, per iteration. Omit
# the demise and the final content keeps one, per call. Let the recorded
# `cell ⊇ content` edge count the holding a second time and the cell's own store
# double-counts it, per iteration again. So the leak faces below vary the
# iteration count: a per-iteration strand reads ~20x a per-call one, which tells
# the channels apart by magnitude and not just by sign.
#
# The over-free face is the mirror. The demise must not fire while the content
# is still reachable — read back inside the loop, read back after it, or handed
# to a container that outlives the cell — and the producer's release at the
# store must not free a value the cell is still holding.
#
# Content produced by a native call and content the solver can name by slot
# (`%pair`) are both driven: the model is one model, and a rate that moved for
# only one of them would name the producer discipline rather than the container.

(defn measure (thunk warm window)
  (var i 0)
  (while (%lt i warm)
    (thunk)
    (assign i (%add i 1)))
  (def before (arena/count))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  (%sub (arena/count) before))

# ── subjects ──────────────────────────────────────────────────────
# CALL-RESULT content carried ACROSS a loop: one assign-value region names a
# different runtime value every iteration, so one release cannot cover them all.
(defn cell-loop-call (n)
  (var last nil)
  (var i 0)
  (while (%lt i n)
    (assign last {:x i})
    (assign i (%add i 1)))
  0)

# The same loop with STATIC-ALLOC content, holding the container fixed and
# varying only what produced the value.
(defn cell-loop-static (n)
  (var last nil)
  (var i 0)
  (while (%lt i n)
    (assign last (%pair i i))
    (assign i (%add i 1)))
  0)

# The final-content face with no loop at all: the cell is written once through a
# branch (which keeps the Assign alive through functionalization) and never
# overwritten, so the ONLY channel that can release its reference is the demise.
(defn cell-final (c)
  (var last nil)
  (if c (assign last {:x 1}) nil)
  0)

# A cell bound INSIDE the loop body — minted per iteration, so its demise must
# fire per iteration too, the opposite hoist from the loop-carried cell above.
(defn cell-inner (n)
  (var i 0)
  (var acc 0)
  (while (%lt i n)
    (let [_ (begin
              (var last nil)
              (assign last {:x i})
              (assign acc (+ acc (get last :x))))]
      _)
    (assign i (%add i 1)))
  acc)

# The content is read back both inside the loop and after it — the latter is why
# the demise hoist is a max over the cell's last access and not a move to the
# loop, since a loop's parameters stay readable past the loop.
(defn cell-read (n)
  (var last nil)
  (var i 0)
  (var sum 0)
  (while (%lt i n)
    (assign last {:x i})
    (assign sum (+ sum (get last :x)))
    (assign i (%add i 1)))
  (+ sum (get last :x)))

# Nested loops over one cell: the hoist must reach the loop that CARRIES the
# cell, not merely the innermost one the store sits in.
(defn cell-nested (n)
  (var last nil)
  (var i 0)
  (var acc 0)
  (while (%lt i n)
    (let [_ (begin
              (var k 0)
              (while (%lt k n)
                (assign last {:x (%add i k)})
                (assign k (%add k 1))))]
      _)
    (assign acc (+ acc (get last :x)))
    (assign i (%add i 1)))
  acc)

# The cell's lifetime spans a fiber PARK: the store, the yield, and the demise
# are in one activation that suspends in between. A release that fires against a
# parked frame's mapping is the failure this drives at.
(defn cell-fiber (n)
  (let [f (fiber/new (fn []
                       (var last nil)
                       (var i 0)
                       (while (%lt i n)
                         (assign last {:x i})
                         (yield (get last :x))
                         (assign i (%add i 1)))
                       (get last :x)) |:yield|)]
    (var sum 0)
    (while (not= (fiber/status f) :dead)
      (let [v (fiber/resume f)]
        (when (int? v) (assign sum (+ sum v)))))
    sum))

# The cell hands its content to a longer-lived container. That store is
# runtime-counted, so the demise must drop only the cell's own reference.
(defn cell-escapes (n keeper)
  (var last nil)
  (var i 0)
  (while (%lt i n)
    (assign last {:x i})
    (push keeper last)
    (assign i (%add i 1)))
  (length keeper))

# ── controls: bounded already ─────────────────────────────────────
# The module-scope 1-slot container (donation + frame teardown) and the same
# loop with no cell at all bracket the diagnosis from the other side.
(var mod-last nil)
(defn mod-churn (n)
  (var i 0)
  (while (%lt i n)
    (assign mod-last {:x i})
    (assign i (%add i 1)))
  0)
(defn no-cell (n)
  (var i 0)
  (while (%lt i n)
    (let [t {:x i}]
      (get t :x))
    (assign i (%add i 1)))
  0)

(def c-mod (measure (fn () (mod-churn 20)) 200 2000))
(def c-none (measure (fn () (no-cell 20)) 200 2000))
(assert (%lt c-mod 400)
        (concat "control: the module-scope 1-slot container strands, delta="
                (number->string c-mod)))
(assert (%lt c-none 400)
        (concat "control: the cell-free loop strands, delta="
                (number->string c-none)))

# ── leak face ─────────────────────────────────────────────────────
(def w-call (measure (fn () (cell-loop-call 20)) 200 2000))
(def w-static (measure (fn () (cell-loop-static 20)) 200 2000))
(def w-final (measure (fn () (cell-final 1)) 200 2000))
(def w-inner (measure (fn () (cell-inner 20)) 200 2000))
(def w-read (measure (fn () (cell-read 20)) 200 2000))
(def w-nested (measure (fn () (cell-nested 6)) 200 1000))
(def w-fiber (measure (fn () (cell-fiber 6)) 200 1000))
(def w-esc
  (measure (fn ()
             (let [k @[]]
               (cell-escapes 20 k))) 200 2000))
(println "region-fn-local-cell-drop-leak deltas over 2000 iters:")
(println "  call-result content, loop:  " w-call)
(println "  static-alloc content, loop: " w-static)
(println "  final content, no loop:     " w-final)
(println "  cell bound inside the loop: " w-inner)
(println "  content read back:          " w-read)
(println "  nested loops over one cell: " w-nested)
(println "  cell across a fiber park:   " w-fiber)
(println "  content stored out:         " w-esc)
(assert (%lt w-call 400)
        (concat "a fn-local cell's displaced call-result content is not released "
                "per iteration, delta=" (number->string w-call)))
(assert (%lt w-static 400)
        (concat "a fn-local cell's displaced static-alloc content is not released "
                "per iteration, delta=" (number->string w-static)))
(assert (%lt w-final 400)
        (concat "a fn-local cell's final content is not released at the cell's "
                "demise, delta=" (number->string w-final)))
(assert (%lt w-inner 400)
        (concat "a cell bound inside the loop body is not dropped per iteration, "
                "delta=" (number->string w-inner)))
(assert (%lt w-read 400)
        (concat "a read-back fn-local cell strands its content, delta="
                (number->string w-read)))
(assert (%lt w-nested 400)
        (concat "a cell carried across NESTED loops strands its content, delta="
                (number->string w-nested)))
(assert (%lt w-fiber 400)
        (concat "a cell whose lifetime spans a fiber park strands its content, "
                "delta=" (number->string w-fiber)))
(assert (%lt w-esc 400)
        (concat "a cell whose content is stored out strands it, delta="
                (number->string w-esc)))

# ── over-free face ────────────────────────────────────────────────
# Every value the cell held must still be readable where the program reads it.
# Under `--trace=guardfree` a premature release detonates at the stale deref;
# the sums catch a silent recycle on the plain tiers.
(var seen 0)
(var k 0)
(while (%lt k 500)
  (assign seen (+ seen (cell-read 4)))
  (assign seen (+ seen (cell-inner 4)))
  (assign seen (+ seen (cell-nested 4)))
  (assign seen (+ seen (cell-fiber 4)))
  (assign k (%add k 1)))
# per iteration: cell-read (0+1+2+3)+3 = 9, cell-inner 0+1+2+3 = 6,
# cell-nested Σ(i+3) for i<4 = 3+4+5+6 = 18, cell-fiber (0+1+2+3)+3 = 9
(assert (%eq seen 21000)
        (concat "a fn-local cell's content did not survive its own reads, sum="
                (number->string seen)))

# The escaping face: the keeper outlives the cell, so the demise must not free
# what the keeper still holds. Read every element back after the call.
(var esc 0)
(var m 0)
(while (%lt m 500)
  (let [keeper @[]]
    (cell-escapes 4 keeper)
    (each e in keeper
      (assign esc (+ esc (get e :x)))))
  (assign m (%add m 1)))
# per iteration: 0+1+2+3 = 6
(assert (%eq esc 3000)
        (concat "a value the cell stored into a longer-lived container did not "
                "survive the cell's demise, sum=" (number->string esc)))

(println "region-fn-local-cell-drop-leak: ok")
