(elle/epoch 12)
# tests/integration/fixtures/region-cell-aliased-init-uaf.lisp
#
# Quarantined here — NOT under tests/elle/ — because an over-free in this shape
# SIGSEGVs under --trace=guardfree, and `make smoke` globs tests/elle/*.lisp into
# one shared process where a segfault would take the whole harness down. It is
# exercised by the guardfree subprocess pin in tests/integration/elle_scripts.rs
# (`region_cell_aliased_init_uaf`). The leak and read-back faces of the same
# shape live in tests/elle/region-cell-aliased-init.lisp.
#
# WHAT IT PINS — the soundness half of the COUNTED INIT
# (docs/impl/region/bindings.md § "What the cell donates it must hold alone;
# what it counts it need not"). A fn-local 1-slot container whose init value
# carries a second name takes that value by a counted store rather than by
# donation, so the alias keeps the producer's reference and the ordinary decref
# that releases it. Donating instead would hand the cell a reference the alias
# still needs, and the cell's first overwrite would free the value under every
# later read of that name.
#
# THE SHAPE'S INGREDIENTS (each is load-bearing):
#   * a HEAP init, so there is a reference to be claimed twice at all;
#   * a SECOND name for the init value, read AFTER the loop, so a donated
#     reference released at the first overwrite is observably gone;
#   * a LOOP over the reassignment — a straight-line fn-local reassign is
#     rewritten into shadowing lets and never reaches the container model;
#   * a FUNCTION boundary, so the cell is fn-local (a module-scope cell is
#     freed by the file-letrec teardown and never reaches this edge);
#   * a CURSOR face, where the value stored each step lives inside the region
#     the alias names, so the store-site release and the alias's release name
#     one region and their order is what the accounting rests on;
#   * BOTH orderings of the alias against the cell binding, which decide which
#     binder allocated the init and therefore which slot carries the producer's
#     release;
#   * enough reps for region ids to recycle onto a freed one — an over-free is
#     state-dependent and reads correctly until the id is reused.

# The alias reads back the head of the chain the cursor walked off.
(defn cursor-read-back [n]
  (let [xs (list 1 2 3 4 5)]
    (var r xs)
    (var seen 0)
    (while (not (empty? r))
      (assign seen (+ seen (first r)))
      (assign r (rest r)))
    (list seen (first xs) (length xs))))

# The alias reads back an init the loop displaced with unrelated values, so the
# displaced init and its replacements live in regions of their own.
(defn churn-read-back [n]
  (let [xs (array 41 42)]
    (var last xs)
    (var i 0)
    (while (< i n)
      (assign last (array i 7))
      (assign i (+ i 1)))
    (list (get xs 1) (get last 1))))

# The keeper face: every value the loop produces also takes a runtime-counted
# funnel store, so a value released twice is freed under a live holder and the
# read below walks a reclaimed page.
(defn churn-keep [n]
  (let [xs (array 41 42)]
    (var out @[])
    (var last xs)
    (var i 0)
    (while (< i n)
      (assign last (array i 7))
      (%array-push out last)
      (assign i (+ i 1)))
    (list (get xs 1) (length out))))

# The alias taken AFTER the cell binding, so the CELL's own binder is what
# allocated the init. The counted store still runs, so the cell's reference and
# the alias's are still distinct; only which slot carries the producer's release
# changes.
(defn alias-after [n]
  (var r (list 1 2 3 4 5))
  (let [keep r]
    (var seen 0)
    (while (not (empty? r))
      (assign seen (+ seen (first r)))
      (assign r (rest r)))
    (list seen (first keep) (length keep))))

(defn drive [reps]
  (var c 0)
  (while (< c reps)
    # Vary the trip count, including zero, so the init reaches the cell both
    # displaced and never displaced.
    (assert (= (cursor-read-back 0) (list 15 1 5)) "cursor read-back")
    (assert (= (churn-read-back (mod c 4)) (list 42 (if (= (mod c 4) 0) 42 7)))
            "churn read-back")
    (assert (= (churn-keep (mod c 3)) (list 42 (mod c 3))) "churn keeper")
    (assert (= (alias-after 0) (list 15 1 5))
            "alias taken after the cell binding")
    (assign c (+ c 1))))

(drive 400)
(println "region-cell-aliased-init-uaf: ok")
