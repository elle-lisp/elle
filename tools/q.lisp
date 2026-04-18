(elle/epoch 8)
(def ox (import "oxigraph"))
(def store (ox:store-open ".elle-mcp/store"))

(def args (drop 1 (sys/args)))
(def result (ox:query store (first args)))
(each row in result
  (def parts @[])
  (each [k v] in (pairs row)
    (push parts (string k "=" (get v 1))))
  (println (string/join (freeze parts) "  ")))
