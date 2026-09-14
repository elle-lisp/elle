(elle/epoch 12)
# audited: 2026-09-14
## tests/http2/modules.lisp — every h2 submodule's own unit tests
##
## One `let*` per module, built from the arguments that module takes, so
## a module whose init signature changed fails here rather than in the
## first program that loads it.

(def huffman ((import "std/http2/huffman")))
(def hpack ((import "std/http2/hpack") :huffman huffman))
(def frame ((import "std/http2/frame")))
(def stream ((import "std/http2/stream") :frame frame))
(def transport ((import "std/http2/transport")))
(def session
  ((import "std/http2/session") :frame frame :stream stream :hpack hpack))
(def reader
  ((import "std/http2/reader") :frame frame :stream stream :hpack hpack
                               :session session))
(def server
  ((import "std/http2/server") :hpack hpack :frame frame :stream stream
                               :session session :reader reader
                               :transport transport))

(huffman:test)
(hpack:test)
(frame:test)
(stream:test)
(session:test)
(reader:test)
(server:test)

(let [m ((import "std/http2"))]
  (m:test))

(println "tests/http2/modules.lisp: all tests passed")
