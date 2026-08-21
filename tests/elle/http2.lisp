(elle/epoch 12)
## tests/elle/http2.lisp — HTTP/2 integration tests
## Submodule tests run inside http2:test (one import, one compilation pass).

(let [m ((import "std/http2"))]
  (m:test))

(println "tests/elle/http2.lisp: all tests passed")
