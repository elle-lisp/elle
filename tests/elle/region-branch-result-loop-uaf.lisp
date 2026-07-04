(elle/epoch 12)
## region/branch-result-loop-uaf — a value produced by a DIFFERENT branch arm on
## different loop iterations, stored into a container, must not be over-freed.
##
## The region analysis treats a branch's result (if/match/cond/and/or) as the
## UNION of its arms' regions, and `emit_decrefs_for` releases EACH such arm
## region at the binding's decref_point by loading the arm's own result slot and
## decreffing that value's region (src/hir/regions.rs `walk`, src/lir/lower/mod.rs
## `emit_decrefs_for`). That is sound in straight-line code: the arm not taken
## never ran, so its slot still holds the entry `nil` and the decref is a no-op.
##
## In a LOOP it is NOT sound. A slot is allocated once and reused every
## iteration. Iteration A takes arm-0 and writes arm-0's slot with a heap value;
## that value then ESCAPES (it is `put` into a table that outlives the loop, so
## the table's insert-incref transfers ownership). Iteration B takes arm-1, so it
## never rewrites arm-0's slot — which still points at iteration A's escaped, and
## still-live, value. The binding's decref_point unconditionally releases arm-0's
## region by loading that stale slot, dropping the escaped value's last reference
## while the table still holds it. The pages recycle and the next deref of the
## table's value hits the arena tag/object-mismatch abort (a use-after-free).
##
## Manifests only "at scale": the freed page must be recycled into a different
## HeapObject before the stale value is read, so the tag mismatch fires. This is
## the defect behind the `parameters.lisp`/`elle test` corpus abort (the runner's
## `lib/sqlite.lisp` read-row builds each row with exactly this shape: a
## `(match (col-type) ...)` value `put` into a `@{}` row, in a per-column loop).
##
## RED before the fix (tag/object-mismatch abort under both tiers); GREEN once the
## decref nils the slot it just released so a later non-taken iteration sees nil.

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
