(elle/epoch 12)
# audited: 2026-09-29
# Two imported instances of the http2 session module mint SETTINGS-ACK latches with distinct futex keys.
# lib/http2.md
#
## The latch key must be unique process-wide, not taken from a module-local
## counter. `(import ...)` returns a fresh module instance each call, so two
## independently-imported session modules each restart a module-local counter
## and hand colliding keys to the scheduler's process-global park-queue.
## Acking one session's SETTINGS then wakes the *other* session's
## settings-waiter (which re-checks its own
## still-zero latch box and re-parks); the intended waiter is never
## woken, so its 30s timeout fires and tears the connection down with a
## spurious SETTINGS_TIMEOUT GOAWAY. tests/lang/sync.lisp pins the same
## collision for lib/sync.
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
## frame/stream/hpack deps — exactly the shape a module-local key collides on.
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
        (string "two independently-imported session instances must mint "
                "distinct SETTINGS-ACK latch keys (process-globally unique)"))

(println "tests/lang/http2-session-futex.lisp: all tests passed")
