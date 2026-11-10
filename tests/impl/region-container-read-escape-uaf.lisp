(elle/epoch 15)
# audited: 2026-10-21
# An element read out of a local Owned container and returned stays live across the caller's read.
#
# docs/impl/escape.md
#
# The sidecar arms guardfree and keeps the JIT off, so the over-free faults at
# the caller's deref.
#
# WHAT IT GUARDS — the container-read ESCAPE face: a heap element READ out of a
# LOCAL OWNED container whose read result then ESCAPES.
#   `(%array-push a (list 1 2))` on a local `@[]` the ownership forest made Owned
#   emits an `AdoptRegion` moving the list into `a`'s Owned subtree (the list is
#   proven interior to `a`). But `(first a)` READS the list back out and the result
#   is RETURNED — so the list ALSO escapes. Escape propagates through the read
#   (`analyze_escape`'s read-result → container-contents edge). An escaping
#   element-read marks the container's stored contents escaping, so the forest
#   refuses to adopt them and the RC path keeps the element live across the
#   caller's read. The counter-factual: without that edge, `a`'s scope-exit
#   subtree drop would free the list under the returned Value, and a later deref
#   of the recycled page would fault. The fault lands only once region ids
#   recycle, so the fixture primes id churn first.
#
# DISTINCT FROM tests/impl/region-pop-tail-moves-out-uaf.lisp. `%pop` REMOVES the
# element, so its funnel EXTRACTS it from the container's subtree
# (`extract_owned_region`). A READ (`first`/`get`/`rest`) borrows — the element
# STAYS in the container — so the answer is not an extract; it is escape
# propagating through the read.
#
# DISTINCT FROM tests/impl/region-container-read-borrow-uaf.lisp, the LOCAL face
# of the same borrow. There the read result never escapes; it is merely used
# AFTER the container's own last mention. What covers it there is the
# container's lifetime (a borrowing read extends the container's decref_point to
# the reader), plus the release order at the shared point. Here the value leaves
# the activation entirely, which no lifetime inside it can bound, so the answer
# is refusal.
#
# The stdlib `push` wrapper grafts to `%push-array-mut` on an @array, which
# shares the raw `%array-push`'s value-into-container edge, so both forms reach
# the same adopt. The raw form is written here because it is the shape under
# test without a graft.

(defn stmt-run [thunk]
  (fn [b]
    (def @i 0)
    (while (< i b)
      (thunk)
      (assign i (+ i 1)))))

# Prime: churn region ids so the freed page below is recycled onto a live region.
((stmt-run (fn []
             (let [a @[]]
               (%array-push a (list 1 2))
               (first a)))) 1000)

# Raw `%array-push` then a RETURNED `(first a)` — the read result escapes the
# container. With the pushed list adopted into it, `a`'s drop would over-free it.
((stmt-run (fn []
             (let [a @[]]
               (%array-push a (list 1 2))
               (first a)))) 1000)

(println "region-container-read-escape-uaf: ok")
