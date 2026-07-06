(elle/epoch 12)
# Repeated-call activation reclamation.
#
# A helper function called many times from a driver reclaims each call's
# activation regions while keeping the callee's own closure region alive. The
# callee closure lives in a letrec forward-reference cell that the driver
# captures by indirection (an uncounted cell store), so the ownership forest
# treats that capture as a BORROW, not a containment: it never adopts the cell
# into the driver's subtree and never claims the callee's region Owned. The
# closure region therefore reclaims on the per-region-RC baseline instead: the
# runtime auto-incref over the driver's env keeps it live across every call, and
# no subtree drop frees it under the live cell.
#
# Two gauges pin the two properties independently (memory.md § 5):
#   - plain VM asserts the repeated calls do not grow live regions without
#     bound (the activation regions ARE reclaimed);
#   - `--trace=guardfree` asserts soundness — no region is freed while a
#     reference into it is still live (a stale deref would fault at the exact
#     access; the invariant is that none occurs).
#
# Self-recursion and mutual recursion reclaim the same way; the case that once
# over-freed was a SEPARATE leaf callee driven in a loop (an acyclic letrec
# forward reference the closure-cycle merge does not cover).

(def checked? (vm/config :checked-intrinsics))

(defn leaf (n)
  42)

# Tail-recursive driver: constant stack, one `leaf` activation per step.
(defn drive (iters)
  (if (%le iters 0)
    0
    (begin
      (leaf iters)
      (drive (%sub iters 1)))))

# Region growth across the driver must not scale with the call count: the
# per-call activation regions reclaim, so a 10x longer drive frees 10x more and
# nets the same bounded residual.
(let* [b1 (arena/region-count)
       _1 (drive 200)
       d200 (%sub (arena/region-count) b1)
       b2 (arena/region-count)
       _2 (drive 2000)
       d2000 (%sub (arena/region-count) b2)]
  (assert (or checked? (%lt d200 20))
          (concat "repeated-call region growth at n=200: delta="
                  (number->string d200)))
  (assert (or checked? (%lt d2000 20))
          (concat "repeated-call region growth at n=2000: delta="
                  (number->string d2000))))

(println "region-repeated-call-adopt-uaf: done")
