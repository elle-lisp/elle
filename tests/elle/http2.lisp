(elle/epoch 10)
## tests/elle/http2.lisp — HTTP/2 integration tests

## ── Submodule tests ──────────────────────────────────────────────────

(let [m ((import "std/http2/huffman"))]
  (m:test))

(let* [h ((import "std/http2/huffman"))
       m ((import "std/http2/hpack") :huffman h)]
  (m:test))

(let [m ((import "std/http2/frame"))]
  (m:test))

(let* [s ((import "std/sync"))
       f ((import "std/http2/frame"))
       m ((import "std/http2/stream") :sync s :frame f)]
  (m:test))

(let* [s ((import "std/sync"))
       f ((import "std/http2/frame"))
       st ((import "std/http2/stream") :sync s :frame f)
       h ((import "std/http2/hpack") :huffman ((import "std/http2/huffman")))
       m ((import "std/http2/session") :sync s :frame f :stream st :hpack h)]
  (m:test))

(let* [s ((import "std/sync"))
       f ((import "std/http2/frame"))
       st ((import "std/http2/stream") :sync s :frame f)
       h ((import "std/http2/hpack") :huffman ((import "std/http2/huffman")))
       sess ((import "std/http2/session") :sync s :frame f :stream st :hpack h)
       m ((import "std/http2/server") :sync s :hpack h :frame f :stream st
                                      :session sess)]
  (m:test))

## ── Full module test (includes loopback) ─────────────────────────────

(let [m ((import "std/http2"))]
  (m:test))

(println "tests/elle/http2.lisp: all tests passed")
