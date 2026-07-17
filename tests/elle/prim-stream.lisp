(elle/epoch 12)
## tests/elle/prim-stream.lisp
## Stream I/O: read-line, read, write

## ── write then read a port ─────────────────────────────────────────
## Scratch file under the platform temp root (with-temp-dir honors TMPDIR and
## cleans up after — no hardcoded paths, no litter). Write and read share one
## thunk so the file's whole lifecycle is self-contained per tier.

(with-temp-dir dir
               (let [fpath (path/join dir "stream")]
                 (let [p (port/open fpath :write)]
                   (port/write p "hello")
                   (port/close p))
                 (let [p (port/open fpath :read)]
                   (let [line (port/read-line p)]
                     (assert (= line "hello")
                             "read-line returns written content"))
                   (port/close p))))

## ── error cases ────────────────────────────────────────────────────

(let [[ok? _] (protect ((fn [] (port/read-line 42))))]
  (assert (not ok?) "read-line non-port errors"))

(let [[ok? _] (protect ((fn [] (eval (read "(port/read-line)")))))]
  (assert (not ok?) "read-line no args errors"))

(println "prim-stream: all tests passed")
