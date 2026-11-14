(elle/epoch 15)
# audited: 2026-11-14
# The window a leak gauge reads its delta over, and the check that bounds the delta.
# tests/impl/overview.md

(defn window-delta [read thunk warm window]
  "Call `thunk` `warm` times, then answer how far `window` more calls move
  the count `read` answers."
  (var i 0)
  (while (< i warm)
    (thunk)
    (assign i (%add i 1)))
  (def before (read))
  (var j 0)
  (while (< j window)
    (thunk)
    (assign j (%add j 1)))
  (- (read) before))

(defn measure [thunk warm window]
  "The objects `window` calls of `thunk` leave live, after `warm` calls."
  (window-delta arena/count thunk warm window))

(defn measure-regions [thunk warm window]
  "The regions `window` calls of `thunk` leave live, after `warm` calls."
  (window-delta arena/region-count thunk warm window))

(defn measure-both [thunk warm window]
  "The objects and the regions `window` calls of `thunk` leave live, after
  `warm` calls, read over one window."
  (var i 0)
  (while (< i warm)
    (thunk)
    (assign i (%add i 1)))
  (def objects (arena/count))
  (def regions (arena/region-count))
  (var j 0)
  (while (< j window)
    (thunk)
    (assign j (%add j 1)))
  [(%sub (arena/count) objects) (%sub (arena/region-count) regions)])

(defn bounded-by [limit]
  "A check that a delta stays under `limit`, which fails naming what leaks."
  (fn [d label] (assert (< d limit) (string label " leaks, delta=" d))))
