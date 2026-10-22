(elle/epoch 15)
# audited: 2026-10-21
# The stdlib copy-scratch family: each shape's per-call rate beside its crest, the high-water mark's slope in its input size.
#
# docs/impl/memory.md
# docs/impl/region/diagnostics.md
#
# A stdlib walk that copies per step frees each copy at the step, or holds
# every copy to the call's return. A per-call rate reads 0 either way, so each
# shape here is read on two scales: `r:drive` for the rate across calls, and
# `r:crest` for the objects live inside one call per element of its input. A
# shape builds its own input, which costs a constant the crest cancels.
#
# `map` and `filter` take `wrap-add` and `wrap-big?`, named functions with a
# `block` body, which a fragment cannot close over, so the stdlib op runs. A
# lambda literal would fuse into a loop and gauge nothing of the op
# (docs/impl/dissolution.md).

(println "── folded suite: stdlib scratch ──")

# The first/rest walk over an array: each `(rest xs)` copies the tail into a
# fresh immutable array, the F1a shape the model names.
(defn walk-rest [xs n]
  (when (%not (%int? n)) (error :n))
  (if (empty? xs) n (walk-rest (rest xs) (%add n 1))))

(r:drive "walk-rest-array" (r:stmt-run (fn [] (walk-rest [1 2 3 4 5 6 7 8] 0)))
         :on [r:objects r:regions])
(r:drive "take-array" (r:stmt-run (fn [] (take 3 [1 2 3 4 5 6 7 8])))
         :on [r:objects r:regions])
(r:drive "drop-array" (r:stmt-run (fn [] (drop 3 [1 2 3 4 5 6 7 8])))
         :on [r:objects r:regions])
(r:drive "each-string"
         (r:stmt-run (fn []
                       (each g in "abcdefgh"
                         (string? g)))) :on [r:objects r:regions])

# The crest. `take` hands back n pairs, so its crest is the result's own; a
# tail slice stranded per step would read 2. Every other shape hands back one
# object or none, so a crest above 0 is a copy the walk kept.
(r:crest "walk-rest-array" (fn [n] (walk-rest (range n) 0)))
(r:crest "take-array" (fn [n] (length (take n (range (* 2 n))))))
(r:crest "drop-array" (fn [n] (length (drop n (range (* 2 n))))))
(r:crest "each-string"
         (fn [n]
           (each g in (string/repeat "a" n)
             (string? g))))
(r:crest "map-unfused" (fn [n] (length (map wrap-add (range n)))))
(r:crest "filter-unfused" (fn [n] (length (filter wrap-big? (range n)))))
