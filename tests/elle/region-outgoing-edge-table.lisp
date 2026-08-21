(elle/epoch 12)
# The outgoing edge table (docs/impl/region/ownership.md § "The outgoing edge table").
# Every cross-region CONTENT edge — a value stored into another region's container —
# is recorded at creation (the alloc funnel + the mutable-store seam) so reclamation
# walks the recorded table instead of scanning page contents. This exercises the seam
# paths that record/un-record edges (push/pop, struct put/del, overwrite, box rebind)
# with genuinely cross-region values, in a loop so each iteration's containers are
# built and discarded — every free runs the `#[cfg(debug_assertions)]` equivalence
# oracle (recorded table == content scan), so accounting drift detonates deterministically.
#
# Correctness here is the value read-back; the deep check is implicit — the oracle in
# a debug build, and `--trace=guardfree` for the UAF a missed/extra edge would cause.
# A missed edge frees a still-referenced region (UAF on read-back / guardfree fault);
# an extra edge over-frees (the oracle panics at the free). Bounded-memory across the
# loop is the leak side. RED if the seam does not record edges in lockstep with RC.

# ── @array probe: push cross-region values, overwrite, pop ────────────
# Each `(list i i)` is a fresh heap value in its own region; pushing it records the
# content edge array-region → element-region; overwrite/pop un-record. The probe
# returns its array (discarded by the caller → freed each iteration).
(defn probe-array [n]
  (var a @[])
  (var i 0)
  (while (%lt i n)
    (push a (list i i))  # records the content edge a → element
    (assign i (%add i 1)))
  (put a 0 (list 999 999))  # overwrite (rebind: un-record old, record new)
  (assert (= (first (get a 0)) 999) "array overwrite element region freed")
  (assert (= (first (get a 5)) 5)
          "array element region freed under the container")
  (pop a)  # un-records the last edge
  a)

# ── @struct probe: put cross-region values, overwrite a key, del ──────
(defn probe-struct [n]
  (var m @{})
  (var k 0)
  (while (%lt k n)
    (put m (string k) (list k k))  # records m → value-region
    (assign k (%add k 1)))
  (put m "0" (list 999 999))  # overwrite (rebind)
  (assert (= (first (get m "0")) 999) "struct overwrite value region freed")
  (assert (= (first (get m "3")) 3)
          "struct value region freed under the container")
  (del m "3")  # un-records its edge
  (assert (not (has? m "3")) "struct del did not remove the key")
  m)

# ── box probe: store a cross-region value, rebind ─────────────────────
(defn probe-box []
  (var b (box (list 1 2)))
  (assert (= (first (unbox b)) 1) "box initial value region freed")
  (rebox b (list 3 4))  # rebind: un-record old, record new
  (assert (= (first (unbox b)) 3) "box rebind value region freed")
  b)

# Drive each probe in a loop: the returned container is discarded by the caller, so
# it (and its recorded edges) frees each iteration — the oracle/guardfree fire on each.
(var p 0)
(while (%lt p 500)
  (probe-array 16)
  (probe-struct 8)
  (probe-box)
  (assign p (%add p 1)))

(println "region-outgoing-edge-table: ok")
