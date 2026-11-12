(elle/epoch 15)
# audited: 2026-11-12
# The http2 module's own tests pass, run inside http2:test in one import and one compilation pass.
# docs/stdlib/http2.md
#

(let [m ((import "std/http2"))]
  (m:test))

(println "tests/lang/http2.lisp: all tests passed")
