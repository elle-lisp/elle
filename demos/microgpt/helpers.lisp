# ── Utility functions for microgpt ────────────────────────────────

(import "plugin/random")

# ── Array shuffling ──────────────────────────────────────────────

(defn shuffle! [arr]
  "Shuffle array in place using Fisher-Yates."
  (var i (- (length arr) 1))
  (while (> i 0)
    (let* ([j (floor (* (random/float) (+ i 1)))]
           [tmp (arr i)])
      (put arr i (arr j))
      (put arr j tmp))
    (assign i (- i 1)))
  arr)

# ── 2D array construction ───────────────────────────────────────

(defn make-2d [rows cols init-fn]
  "Create a rows x cols mutable 2D array, calling (init-fn r c) for each cell."
  (let* ([result @[]])
    (each r in (range rows)
      (let* ([row @[]])
        (each c in (range cols)
          (push row (init-fn r c)))
        (push result row)))
    result))

# ── KV cache construction ───────────────────────────────────────

(defn make-kv-caches [n-layer]
  "Create fresh per-layer KV caches. Returns [keys-cache values-cache]."
  (let* ([ks @[]] [vs @[]])
    (each _ in (range n-layer)
      (push ks @[])
      (push vs @[]))
    [ks vs]))
