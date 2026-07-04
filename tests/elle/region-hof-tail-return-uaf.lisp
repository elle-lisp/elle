(elle/epoch 12)
# Regression guard (GREEN — fixed): a function whose TAIL expression is a stdlib
# higher-order call over a collection — `(map f xs)` / `(filter p xs)` — used to
# tail-return the HOF's freshly built heap result WITHOUT a ReturnValue retain, so
# the caller's `DecrefValueRegion` freed it under the caller's borrow: a
# use-after-free. The native-tail post-block now emits that retain, so the result
# survives; this script locks the retain against regression.
#
# This is the HOF manifestation of region-native-tail-return-uaf.lisp — the SAME
# native-tail return-convention defect (the post-`TailCall` `Return` omits the
# heap-result retain), here surfacing through `map`/`filter` instead of
# `first`/`get`/call-index. It is the minimal form of the dns/parse-resolv-conf
# regression: `(defn parse … (freeze (filter … (map … (string/split …)))))` — a
# pipeline whose TAIL is a HOF call — over-releases the returned collection. It
# is NOT a "filter" bug: bisection shows consuming the HOF result with a borrowing
# native (`length`) is correct (the controls below), and `map` over IMMEDIATES
# faults identically, so the freed region is the returned COLLECTION, not its
# elements. Pinned separately from the primitive-accessor pin because the
# real-world shape (a HOF-terminated pipeline) is the one that bit us, and a
# future fix must green BOTH surfaces.
#
# Run under `--trace=guardfree`, the robust oracle: before the fix a regression
# here is deterministic (the freed page is mprotected, so the FIRST tail-returned
# subject's loop faults immediately rather than reading back an intact recycled
# page). Each subject runs in its OWN loop (no interleaving); the controls run
# first and must stay correct.
#
# Loop sizing: each loop must comfortably cross the adaptive-JIT hotness
# threshold (10 calls) so the compile-window property is exercised, while the
# WHOLE file stays within the kernel mapping budget (vm.max_map_count) — the
# guardfree oracle leaks one PROT_NONE mapping per freed region page, so a
# reclaiming stdlib-heavy loop consumes mappings in proportion to its iteration
# count (~56/iteration here). 500 iterations is 50x the threshold at well under
# half the default 65530-map budget.
#
# Fixed by the native-tail ReturnValue retain (commit 434d9d1f; splice-tail in
# 56781a2c): the post-`TailCall` `Return` now retains the heap result before
# `Return`. GREEN under guardfree; a regression that drops the retain SIGSEGVs in
# the first subject's loop. docs/impl/region-rules.md Rules 4, 5, 8.

# ── controls: HOF result CONSUMED by a borrowing native, not tail-returned ──────
(defn ctl_map (xs)
  (length (map (fn (a) a) xs)))
(defn ctl_filter (xs)
  (length (filter (fn (a) a) xs)))

(var c 0)
(var rc 0)
(while (%lt c 500)
  (assign rc (ctl_map [(concat "a" "a") (concat "b" "b")]))
  (assign rc (ctl_filter [(concat "a" "a") (concat "b" "b")]))
  (assign c (%add c 1)))
(assert (= rc 2) "control: borrowing-consumer HOF mis-read (harness broken)")

# ── subjects: tail-return the HOF's heap result ────────────────────────────────
(defn ret_map (xs)
  (map (fn (a) a) xs))
(defn ret_filter (xs)
  (filter (fn (a) a) xs))

# subject 1: map tail-return (own loop — deterministic guardfree fault here)
(var i 0)
(var m nil)
(while (%lt i 500)
  (assign m (ret_map [(concat "a" "a") (concat "b" "b")]))
  (assign i (%add i 1)))
(assert (= (length m) 2)
        "(map f xs) tail-returned: HOF heap result freed under the caller's borrow")

# subject 2: filter tail-return (own loop)
(var j 0)
(var fl nil)
(while (%lt j 500)
  (assign fl (ret_filter [(concat "a" "a") (concat "b" "b")]))
  (assign j (%add j 1)))
(assert (= (length fl) 2)
        "(filter p xs) tail-returned: HOF heap result freed under the caller's borrow")

(println "region-hof-tail-return-uaf: ok")
