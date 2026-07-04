(elle/epoch 12)
# tests/elle/region-toplevel-mutable-reassign.lisp
#
# KNOWN-RED counterfactual cornering the live phantom-region / double-free
# (regionstore.rs:172 "DecrefRegion(N) … phantom region or double-free").
# `advanced.lisp` bottoms out here (its lines 750-758 accumulate `(pair i
# test-result)` into a top-level `(def @test-result (list))` in an `each`).
#
# BOUNDARY (all deterministic, --jit=off):
#   - A TOP-LEVEL mutable binding (`def @x` OR `var x`) ...
#   - reassigned via `assign` to a HEAP value (pair/list/array/string/closure;
#     an immediate int is FINE — no region) ...
#   - followed by ANY subsequent top-level statement.
#   The trailing statement is load-bearing: the `assign`'d heap value's region
#   is freed while the binding still holds it (premature), then the NEXT
#   top-level statement's region teardown decrefs the recycled/freed id again
#   → phantom-region abort. With NO trailing statement the second decref never
#   fires, which is why the bare repro `(def @r (list)) (assign r (pair 1 2))`
#   *appears* to pass — it does not; it is just the last thing that runs.
#   A local `@`/`let` binding inside a fn is FINE (its scope owns the store).
#
# THEORY (verify against this repro before fixing): `handle_store_local`
# (vm/variables.rs) does ZERO region accounting on an `assign` to a top-level
# mutable local — no incref-on-store, no decref-of-old — unlike the capture
# stores `handle_store_upvalue`/`handle_update_capture` (which do `CaptureStore`
# incref + old decref). So the stored heap value is unowned, yet the lowerer
# still emits a scope `DecrefRegion` for it. Fix is one of: give the top-level
# mutable store an owning incref (mirror `CaptureStore`), or stop the lowerer
# emitting that scope `DecrefRegion` for a value escaping into the binding.
#
# This file is EXPECTED TO ABORT until the double-free is fixed; it then must
# pass. Do not "green" it by deleting the trailing statements — that hides the
# bug it exists to catch.

# ── 1. def @ : reassign to a heap pair, then keep running ───────────────
(def @r (list))
(assign r (pair 1 2))
(assert (= r (pair 1 2)) "def @ reassigned to a heap pair reads back correctly")

# ── 2. var : same, with a non-@ mutable binding ─────────────────────────
(var v (list))
(assign v (pair 3 4))
(assert (= v (pair 3 4)) "var reassigned to a heap pair reads back correctly")

# ── 3. repeated self-referential accumulation in a loop (the advanced case)
(def @acc (list))
(each i (list 1 2 3)
  (assign acc (pair i acc)))
(assert (= (reverse acc) (list 1 2 3))
        "loop self-ref accumulation into a top-level @ preserves all elements")

# ── 4. reassign to other heap kinds, each followed by a use ─────────────
(def @s "")
(assign s (concat "ab" "cd"))
(assert (= s "abcd") "def @ reassigned to a heap string reads back correctly")

(def @a (list))
(assign a (@array 1 2 3))
(assert (= (length a) 3) "def @ reassigned to a heap array reads back correctly")

(println "region-toplevel-mutable-reassign: OK")
