(elle/epoch 13)
# audited: 2026-09-29
# A function whose tail is `(map f xs)` or `(filter p xs)` hands its caller a
# collection the caller can read.
#
# docs/impl/region/rules.md
#
# The tail is a native call, so the frame returns the HOF's fresh result
# through the post-`TailCall` `Return`, which must retain it. Without the
# retain the caller's `DecrefValueRegion` frees the collection under its own
# borrow. region-native-tail-return-uaf.lisp pins the same return path through
# `first` and `get`; this file pins it through a HOF, and `map` over immediates
# fails the same way, so the freed region is the collection, not its elements.
#
# The controls consume the HOF's result with a borrowing native (`length`)
# instead of returning it, and must stay correct: they separate a broken
# harness from a broken return.
#
# Run under `--trace=guardfree`. A freed page is then unmapped, so the first
# subject's loop faults at once instead of reading back a recycled page.
#
# The trap is the loop size. Each loop must pass the adaptive JIT threshold
# (10 calls), and guardfree keeps one PROT_NONE mapping per freed page, about
# 56 per iteration here, against the kernel's vm.max_map_count (65530 by
# default). 500 iterations crosses the threshold 50 times over and stays under
# half that budget. Each subject runs in its own loop, after the controls.

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
