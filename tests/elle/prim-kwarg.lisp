(elle/epoch 12)
## tests/elle/prim-kwarg.lisp
## Keyword argument handling (tested indirectly through port/open etc)

## timeout keyword extraction is tested via port/open
(let [p (port/open "/tmp/elle-test-kwarg" :write :timeout 5000)]
  (assert (port? p) "timeout kwarg accepted")
  (port/close p))

(let [p (port/open "/tmp/elle-test-kwarg" :write)]
  (assert (port? p) "no timeout kwarg ok")
  (port/close p))

(let [[ok? _] (protect ((fn [] (port/open "/tmp/foo" :write :timeout -1))))]
  (assert (not ok?) "negative timeout errors"))

(let [[ok? _] (protect ((fn [] (port/open "/tmp/foo" :write :timeout "foo"))))]
  (assert (not ok?) "bad timeout type errors"))

(let [[ok? _] (protect ((fn [] (port/open "/tmp/foo" :write :unknown 1))))]
  (assert (not ok?) "unknown keyword errors"))

(println "prim-kwarg: all tests passed")
