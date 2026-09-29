(elle/epoch 12)
# audited: 2026-09-29
# Keyword arguments reach a primitive, tested through port/open.
# docs/named-args.md
#

# Scratch dir for the fixture files; removed at the bottom of the file.
(def scratch (file/mktempdir))
(def kwarg-path (path/join scratch "kwarg"))
(def foo-path (path/join scratch "foo"))

## timeout keyword extraction is tested via port/open
(let [p (port/open kwarg-path :write :timeout 5000)]
  (assert (port? p) "timeout kwarg accepted")
  (port/close p))

(let [p (port/open kwarg-path :write)]
  (assert (port? p) "no timeout kwarg ok")
  (port/close p))

(let [[ok? _] (protect ((fn [] (port/open foo-path :write :timeout -1))))]
  (assert (not ok?) "negative timeout errors"))

(let [[ok? _] (protect ((fn [] (port/open foo-path :write :timeout "foo"))))]
  (assert (not ok?) "bad timeout type errors"))

(let [[ok? _] (protect ((fn [] (port/open foo-path :write :unknown 1))))]
  (assert (not ok?) "unknown keyword errors"))

(file/delete-dir-all scratch)
(println "prim-kwarg: all tests passed")
