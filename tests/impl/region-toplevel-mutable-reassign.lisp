(elle/epoch 13)
# audited: 2026-09-29
# A top-level mutable reassigned to a heap value keeps that value live through every later top-level statement.
# docs/impl/region/bindings.md
#
# BOUNDARY:
#   - A TOP-LEVEL mutable binding (`def ` OR `var x`) ...
#   - reassigned via `assign` to a HEAP value (pair/list/array/string/closure;
#     an immediate int has no region) ...
#   - followed by ANY later top-level statement.
#   The trailing statement matters: a value released while the binding still
#   holds it is freed a second time by the next statement's region teardown, a
#   phantom-region abort in the regionstore. With NO trailing statement the
#   second decref never fires, so a bare `(def  (list)) (assign r (pair 1 2))`
#   passes whether or not the release is right. A local `@`/`let` binding inside
#   a fn is outside this shape: its scope owns the store.
#
# The module-scope cell adopts the producer's reference, and drop-on-overwrite releases
# each displaced value (docs/impl/region/bindings.md).
# `RegionInfo::mutated_binding_value_regions` keeps a value-routed release off the
# reassigned slot. tests/lang/advanced.lisp accumulates `(pair i test-result)` into a
# top-level `(def -result (list))` in an `each`, the everyday form of case 3.
#
# Do not "green" a regression by deleting the trailing statements — that hides
# the fault this file exists to catch.

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
