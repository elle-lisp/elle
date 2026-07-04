(elle/epoch 12)
# Counterfactual: the trait-registry default `:Collection :conj` stores into a
# MUTABLE collection (@array push / @set insert, `trait_coll_conj` in
# src/primitives/traitregistry/methods.rs) WITHOUT the Rule-5 mutable-store
# incref (docs/impl/region-rules.md Rule 5; `incref_inserted_element` in value/arena.rs).
# The stored element's producing region is released at its statement's decref
# point — RC reaches 0 with the container still holding the value, and the
# next read is a use-after-free.
#
# Witness in a debug build: the read-back `get` performs the pass-through
# retain, whose `region_of` trips the region-generation check on the freed
# page — a deterministic "stale region deref" panic at the read (docs/
# docs/impl/region-generations.md § "Region generations"). Without generation tags the same defect
# is a wrong-value read or a guardfree fault.
#
# Control: `push` routes through the tracked funnel (`push_with_incref`) and
# the same shape is correct NOW — the bisection naming conj's raw store as
# the culprit.
#
# A UAF, NOT a leak. RED before trait_coll_conj routes its mutable-container
# stores through the arena funnels; GREEN after.

(def conj-fn (((traits @[0]) :Collection) :conj))

# ── control: push (tracked funnel), same shape ────────────────────
(def ctl @[])
(var c 0)
(while (%lt c 2000)
  (push ctl (list c c))
  (assign c (%add c 1)))
(var cj 0)
(var ctl-ok true)
(while (%lt cj 2000)
  (when (not (= (first (get ctl cj)) cj)) (assign ctl-ok false))
  (assign cj (%add cj 1)))
(assert ctl-ok "control: push-stored element mis-read (harness broken)")

# ── witness 1: @array conj stores a fresh heap value each iteration ─
(def acc @[])
(var i 0)
(while (%lt i 2000)
  (conj-fn acc (list i i))
  (assign i (%add i 1)))
(var j 0)
(var arr-ok true)
(while (%lt j 2000)
  (when (not (= (first (get acc j)) j)) (assign arr-ok false))
  (assign j (%add j 1)))
(assert arr-ok
        "@array conj store is uncounted — element region freed under the container")

# ── witness 2: @set conj stores a fresh heap string each iteration ─
(def st @||)
(var k 0)
(while (%lt k 500)
  (conj-fn st (concat "v" (string k)))
  (assign k (%add k 1)))
(var m 0)
(var set-ok true)
(while (%lt m 500)
  (when (not (has? st (concat "v" (string m)))) (assign set-ok false))
  (assign m (%add m 1)))
(assert set-ok
        "@set conj store is uncounted — element region freed under the container")

(println "region-conj-store: ok")
