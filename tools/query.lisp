(elle/epoch 7)
## query.lisp — run a SPARQL query against the store
## Usage: tools/run-elle.sh tools/query.lisp -- 'SELECT ...'

(def ox (import "oxigraph"))
(def store (ox:store-open ".elle-mcp/store"))

(def args (drop 1 (sys/args)))
(when (empty? args)
  (eprintln "usage: query.lisp -- 'SPARQL query'")
  (error {:error :usage :message "no query"}))

(def result (ox:query store (first args)))
(each row in result
  (def parts @[])
  (each [k v] in (pairs row)
    (push parts (string k "=" (get v 1))))
  (println (string/join (freeze parts) "  ")))
