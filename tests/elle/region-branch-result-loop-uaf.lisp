(elle/epoch 12)
# audited: 2026-09-23
# A value a different branch arm produces on each loop iteration, stored into a container, is never over-freed.
# docs/impl/region/anchors.md
#
# The region analysis treats a branch's result (if/match/cond/and/or) as the
# UNION of its arms' regions. `emit_decrefs_for` (src/lir/lower/regiondecref.rs)
# releases EACH arm region at the binding's decref_point by loading the arm's
# own result slot and releasing that value's region. In straight-line code the
# arm not taken never ran, so its slot holds the entry `nil` and the release is
# a no-op.
#
# THE TRAP is a LOOP. A slot is allocated once and reused every iteration.
# Iteration A takes arm-0 and writes arm-0's slot with a heap value, which then
# ESCAPES into a table that outlives the loop. Iteration B takes arm-1 and never
# rewrites arm-0's slot. So the release stamps the slot `nil` after it reads it.
# Counterfactual: without the stamp, iteration B's release loads iteration A's
# escaped value from the stale slot and drops its last reference while the table
# still holds it.
#
# The fault shows only at scale: the freed page must be recycled into a
# different HeapObject before the stale value is read. `lib/sqlite.lisp`'s
# read-row has this shape: a `(match (col-type) ...)` value `put` into a `@{}`
# row, in a per-column loop.

## read-row's shape, distilled: a per-key loop whose value comes from a match that
## takes a different (heap-allocating) arm on alternating iterations, each `put`
## into a fresh table that is frozen and kept alive in `rows`.
(defn make-row [n]
  (let [row @{}]
    (each ci in (range 4)
      (let [val (match (mod ci 2)
                  0 (concat "even-" (number->string n) "-" (number->string ci))
                  _ (concat "odd-" (number->string n) "-" (number->string ci)))]
        (put row (keyword (concat "c" (number->string ci))) val)))
    (freeze row)))

(def rows @[])
(each n in (range 80)
  (push rows (make-row n)))

## Churn the heap so any prematurely-freed page is recycled into a new object
## before we read the rows back — this is what turns the latent over-free into
## the observable tag/object-mismatch.
(each n in (range 20000)
  (concat "churn-" (number->string n)))

## Every stored value must still read back BYTE-EXACT. A surviving over-free
## either corrupts the bytes (assert fails) or trips the arena UAF detector.
(def @ok true)
(each n in (range 80)
  (let [r (get rows n)]
    (each ci in (range 4)
      (let [want (concat (if (= 0 (mod ci 2)) "even-" "odd-") (number->string n)
                         "-" (number->string ci))
            got (get r (keyword (concat "c" (number->string ci))))]
        (unless (= got want) (assign ok false))))))

(assert ok
        "branch-result values put into a per-iteration table survive the loop")
(println "region-branch-result-loop-uaf: ok")
