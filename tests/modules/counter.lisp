# Module that returns a fresh counter on each import.
# If import-file caches, both imports share the same counter.
(var count 0)
(defn inc [] (assign count (+ count 1)) count)
(fn [] {:inc inc :count (fn [] count)})
