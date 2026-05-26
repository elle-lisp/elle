(elle/epoch 11)
## tests/elle/prim-ports.lisp
## Port lifecycle, open flags, seek/tell, set-options

## ── port/open ──────────────────────────────────────────────────────

# port/open with :write mode creates a file
(let [p (port/open "/tmp/elle-test-prim-ports-write" :write)]
  (assert (port? p) "port/open :write returns a port")
  (port/close p))

# port/open with :read mode
(let [[ok? err] (protect ((fn []
                            (port/open "/tmp/elle-test-prim-ports-write" :read))))]
  (assert ok? "port/open :read on existing file succeeds"))

# port/open-bytes
(let [p (port/open-bytes "/tmp/elle-test-prim-ports-bytes" :write)]
  (assert (port? p) "port/open-bytes returns a port")
  (port/close p))

# port/open with :timeout keyword
(let [p (port/open "/tmp/elle-test-prim-ports-timeout" :write :timeout 5000)]
  (assert (port? p) "port/open with :timeout succeeds")
  (port/close p))

## ── port/open error cases ──────────────────────────────────────────

(let [[ok? _] (protect ((fn [] (port/open 42 :read))))]
  (assert (not ok?) "port/open: non-string path errors"))

(let [[ok? _] (protect ((fn [] (port/open "/tmp/foo" :badmode))))]
  (assert (not ok?) "port/open: bad mode errors"))

(let [[ok? _] (protect ((fn [] (port/open "/tmp/foo" "read"))))]
  (assert (not ok?) "port/open: non-keyword mode errors"))

(let [[ok? _] (protect ((fn [] (port/open "/tmp/foo" :read :timeout -1))))]
  (assert (not ok?) "port/open: negative timeout errors"))

(let [[ok? _] (protect ((fn [] (port/open "/tmp/foo" :read :unknown 100))))]
  (assert (not ok?) "port/open: unknown keyword errors"))

## ── port/set-options ───────────────────────────────────────────────

(let [p (port/open "/tmp/elle-test-prim-ports-opts" :write)]
  (port/set-options p :timeout 5000)
  (port/set-options p :timeout nil)
  (port/close p))

(let [[ok? _] (protect ((fn [] (port/set-options 42 :timeout 1))))]
  (assert (not ok?) "port/set-options: non-port errors"))

## ── port/path ──────────────────────────────────────────────────────

(let [[ok? _] (protect ((fn [] (port/path 42))))]
  (assert (not ok?) "port/path: non-port errors"))

## ── port/seek and port/tell ────────────────────────────────────────

# Write a file, then seek and tell on it
(let [p (port/open "/tmp/elle-test-seek-tell" :write)]
  (port/write p "hello")
  (port/close p))

(let [p (port/open "/tmp/elle-test-seek-tell" :read)]
  (port/seek p 0)
  (port/seek p 0 :from :start)
  (port/seek p 0 :from :current)
  (port/seek p 0 :from :end)
  (port/close p))

(let [[ok? _] (protect ((fn [] (port/seek 42 0))))]
  (assert (not ok?) "port/seek: non-port errors"))

(let [[ok? _] (protect ((fn [] (port/tell 42))))]
  (assert (not ok?) "port/tell: non-port errors"))

(println "prim-ports: all tests passed")
