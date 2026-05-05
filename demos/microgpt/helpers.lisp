(elle/epoch 10)
## helpers.lisp — Utility functions for microgpt

(fn []
  (defn make-2d [rows cols init-fn]
    "Create a rows x cols mutable 2D array, calling (init-fn r c) for each cell."
    (let* [[result @[]]]
      (each r in (range rows)
        (let* [[row @[]]]
          (each c in (range cols)
            (push row (init-fn r c)))
          (push result row)))
      result))

  (defn make-kv-caches [n-layer]
    "Create fresh per-layer KV caches. Returns [keys-cache values-cache]."
    (let* [[ks @[]] [vs @[]]]
      (each _ in (range n-layer)
        (push ks @[])
        (push vs @[]))
      [ks vs]))

  {:make-2d make-2d :make-kv-caches make-kv-caches})
