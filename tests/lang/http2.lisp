(elle/epoch 13)
# audited: 2026-09-29
# The http2 module's own tests pass, run inside http2:test in one import and one compilation pass.
# lib/http2.md
#

(let [m ((import "std/http2"))]
  (m:test))

(println "tests/lang/http2.lisp: all tests passed")
