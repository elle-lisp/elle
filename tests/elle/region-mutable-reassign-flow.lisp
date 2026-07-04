(elle/epoch 12)
# tests/elle/region-mutable-reassign-flow.lisp
#
# KNOWN-RED. Straight-line / data-flow facets of the file-letrec mutable-
# reassign bug that the reaching-definitions fix must get right. Companions:
#   region-toplevel-mutable-reassign.lisp  — the minimal single-reassign repro
#   region-mutable-reassign-selfref.lisp   — self-referential accumulation
#   region-mutable-reassign-branch.lisp    — conditional (phi) reassignment
#   region-mutable-reassign-scoped.lisp    — cases that are ALREADY correct
#
# WHY EACH ASSERT IS HARDENED. A premature/double free in this area is often
# a LATENT use-after-free: it is guardfree-detectable (`--trace=guardfree`)
# but, in plain `--jit=off` mode, the freed page is usually not yet reused so
# the program reads stale-but-intact bytes and *passes* — a false green that
# `make smoke` would not catch. So each facet manifests the fault
# deterministically in normal mode, by one of:
#   (a) a TRAILING top-level statement, so a deferred double-free fires its
#       second `DecrefRegion` on the recycled id (the regionstore phantom); or
#   (b) ALIAS the value into a longer-lived structure, then force region
#       recycling with junk allocations, then read the alias — if the value's
#       region was freed early, the alias now reads garbage (wrong value).
# Verified on the current tree: every facet below faults (crash or wrong
# value) deterministically; the fix must turn them all green.
#
# Distinct binding names per facet so file-letrec bindings don't interact.

# ── 1. repeated reassign (non-self-ref): each prior value dies at its
#       overwrite; only the last reaches the read ────────────────────────
(def @ra (list))
(assign ra (pair 1 2))
(assign ra (pair 3 4))
(assign ra (pair 5 6))
(assert (= ra (pair 5 6)) "repeated reassign keeps the last value")
(println "flow-1 ok")

# ── 2. reads BETWEEN assigns: each value must be live until the next
#       overwrite ───────────────────────────────────────────────────────
(def @rb (list))
(assign rb (pair 1 2))
(assert (= rb (pair 1 2)) "value live before next overwrite")
(assign rb (pair 3 4))
(assert (= rb (pair 3 4)) "second value live after overwrite")
(println "flow-2 ok")

# ── 3. two distinct top-level mutables, interleaved reassigns ───────────
(def @rc (list))
(def @rd (list))
(assign rc (pair 1 2))
(assign rd (pair 3 4))
(assign rc (pair 5 6))
(assert (= rc (pair 5 6)) "first mutable holds its last value")
(assert (= rd (pair 3 4)) "second mutable is undisturbed")
(println "flow-3 ok")

# ── 4. cross-region chain: the stored value references ANOTHER file-letrec
#       binding's region (a real cross-region edge) ─────────────────────
(def @re (pair 1 2))
(def @rf (list))
(assign rf (pair 9 re))
(assert (= (first rf) 9) "cross-region: head preserved")
(assert (= (rest rf) (pair 1 2)) "cross-region: referenced region preserved")
(println "flow-4 ok")

# ── 5. immediate init then heap reassign (the heap value escapes into the
#       binding). Latent UAF — manifested via alias+recycle ─────────────
(def @rg 0)
(assign rg (pair 1 2))
(def keep-g (list rg rg rg))
(def junk-g (list (pair 8 8) (pair 9 9) (pair 8 8) (pair 9 9)))
(assert (= (first keep-g) (pair 1 2))
        "heap value reassigned over an immediate survives later allocations")
(println "flow-5 ok")

# ── 6. reassign across heap kinds; final must survive. Latent UAF —
#       manifested via alias+recycle ───────────────────────────────────
(def @rh "")
(assign rh (concat "ab" "cd"))
(assign rh (@array 1 2 3))
(assign rh (list 7 8))
(def keep-h (list rh rh rh))
(def junk-h (list (pair 8 8) (pair 9 9) (pair 8 8) (pair 9 9)))
(assert (= (first keep-h) (list 7 8))
        "final value survives reassigns across string/array/list kinds")
(println "flow-6 ok")

(println "region-mutable-reassign-flow: OK")
