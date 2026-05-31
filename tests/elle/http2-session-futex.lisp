(elle/epoch 10)
## tests/elle/http2-session-futex.lisp
##
## Regression: the HTTP/2 session SETTINGS-ACK latch must use a
## process-globally-unique futex key, not a module-local counter.
##
## `lib/http2/session.lisp` minted the latch key from a module-local
## `*session-futex-id*` counter.  `(import ...)` returns a fresh module
## instance each call, so two independently-imported session modules
## restarted that counter and handed colliding keys to the scheduler's
## process-global park-queue.  Acking one session's SETTINGS would then
## wake the *other* session's settings-waiter (which re-checks its own
## still-zero latch box and re-parks); the intended waiter is never
## woken, so its 30s timeout fires and tears the connection down with a
## spurious SETTINGS_TIMEOUT GOAWAY.  (Same bug class as the lib/sync
## futex-key collision pinned by tests/elle/sync.lisp §11.)
##
## Keys must be unique across module instances.  Tested at the latch
## layer: two session instances minting two SETTINGS-ACK latches must
## not share a key.  The main fiber only inspects state (never parks);
## the 30s timeout-waiter fibers spawned by send-settings stay parked
## and are aborted at teardown.

(def huffman ((import "std/http2/huffman")))
(def hpack ((import "std/http2/hpack") :huffman huffman))
(def frame ((import "std/http2/frame")))
(def stream ((import "std/http2/stream") :frame frame))

## Two INDEPENDENT imports of the session module, sharing the same
## frame/stream/hpack deps — exactly the shape that collides under the bug.
(def sessionA
  ((import "std/http2/session") :frame frame :stream stream :hpack hpack))
(def sessionB
  ((import "std/http2/session") :frame frame :stream stream :hpack hpack))

(def mock-transport {:read nil :write nil :flush nil :close nil})
(def sA (sessionA:make-session mock-transport "test" false))
(def sB (sessionB:make-session mock-transport "test" false))

## Each send-settings mints a SETTINGS-ACK latch and stows it on the
## session as {:key K :box B}.
(sessionA:send-settings sA sessionA:default-settings)
(sessionB:send-settings sB sessionB:default-settings)

(assert (not (nil? sA:settings-ack-latch)) "sA minted a settings-ack latch")
(assert (not (nil? sB:settings-ack-latch)) "sB minted a settings-ack latch")
(assert (not (= sA:settings-ack-latch:key sB:settings-ack-latch:key))
        "two independently-imported session instances must mint distinct SETTINGS-ACK latch keys (process-globally unique)")

(println "tests/elle/http2-session-futex.lisp: all tests passed")
