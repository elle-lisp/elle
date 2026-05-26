(elle/epoch 11)
## tests/elle/prim-stream.lisp
## Stream I/O: read-line, read, write

## ── write to a port ────────────────────────────────────────────────

(let [p (port/open "/tmp/elle-test-stream" :write)]
  (port/write p "hello")
  (port/close p))

## ── read from a port ───────────────────────────────────────────────

(let [p (port/open "/tmp/elle-test-stream" :read)]
  (let [line (port/read-line p)]
    (assert (= line "hello") "read-line returns written content"))
  (port/close p))

## ── error cases ────────────────────────────────────────────────────

(let [[ok? _] (protect ((fn [] (port/read-line 42))))]
  (assert (not ok?) "read-line non-port errors"))

(let [[ok? _] (protect ((fn [] (eval (read "(port/read-line)")))))]
  (assert (not ok?) "read-line no args errors"))

(println "prim-stream: all tests passed")
