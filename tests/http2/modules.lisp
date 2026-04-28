(elle/epoch 9)
## tests/http2/modules.lisp — submodule unit tests

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
       m ((import "std/http2/server") :sync s :hpack h :frame f :stream st :session sess)]
  (m:test))

(let [m ((import "std/http2"))]
  (m:test))

(println "tests/http2/modules.lisp: all tests passed")
