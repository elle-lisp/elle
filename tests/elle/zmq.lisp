(elle/epoch 9)

## ZMQ FFI library integration tests
## Tests lib/zmq.lisp (FFI bindings to system libzmq)
##
## Uses inproc:// transport — no network access needed.
## Skipped if libzmq.so is not installed.

(let [[ok? _] (protect (ffi/native "libzmq.so"))]
  (unless ok?
    (println "zmq tests skipped: libzmq.so not found")
    (sys/exit 0)))

(def zmq ((import-file "lib/zmq.lisp")))

## ── Context creation ─────────────────────────────────────────────

(def ctx (zmq:context))
(assert (not (nil? ctx)) "context is not nil")

## ── REQ/REP over inproc:// ───────────────────────────────────────

(let* [rep (zmq:socket ctx :rep)
       req (zmq:socket ctx :req)]
  (zmq:bind rep "inproc://test-reqrep")
  (zmq:connect req "inproc://test-reqrep")

  # Send from REQ, receive on REP
  (zmq:send req "hello")
  (let [msg (zmq:recv-string rep)]
    (assert (= msg "hello") "REQ->REP: received 'hello'"))

  # Reply from REP, receive on REQ
  (zmq:send rep "world")
  (let [msg (zmq:recv-string req)]
    (assert (= msg "world") "REP->REQ: received 'world'"))

  (zmq:close req)
  (zmq:close rep))

## ── PUSH/PULL over inproc:// ─────────────────────────────────────

(let* [pull (zmq:socket ctx :pull)
       push (zmq:socket ctx :push)]
  (zmq:bind pull "inproc://test-pushpull")
  (zmq:connect push "inproc://test-pushpull")

  (zmq:send push "msg1")
  (zmq:send push "msg2")

  (assert (= (zmq:recv-string pull) "msg1") "PUSH/PULL msg1")
  (assert (= (zmq:recv-string pull) "msg2") "PUSH/PULL msg2")

  (zmq:close push)
  (zmq:close pull))

## ── PUB/SUB over inproc:// ───────────────────────────────────────

(let* [pub (zmq:socket ctx :pub)
       sub (zmq:socket ctx :sub)]
  (zmq:bind pub "inproc://test-pubsub")
  (zmq:subscribe sub "")
  (zmq:connect sub "inproc://test-pubsub")

  # Give the subscription time to propagate
  # (inproc is synchronous but SUB needs a moment)
  (zmq:set-option sub :rcvtimeo 500)

  # Send several messages — first few may be lost before subscription completes
  (each _ in (range 10)
    (zmq:send pub "broadcast"))

  (let [msg (zmq:recv-string sub)]
    (assert (= msg "broadcast") "PUB/SUB received broadcast"))

  (zmq:close sub)
  (zmq:close pub))

## ── Multipart messages ───────────────────────────────────────────

(let* [rep (zmq:socket ctx :rep)
       req (zmq:socket ctx :req)]
  (zmq:bind rep "inproc://test-multipart")
  (zmq:connect req "inproc://test-multipart")

  (zmq:send-multipart req ["frame1" "frame2" "frame3"])

  (let [frames (zmq:recv-multipart rep)]
    (assert (= (length frames) 3) "multipart: 3 frames")
    (assert (= (string (get frames 0)) "frame1") "multipart: frame1")
    (assert (= (string (get frames 1)) "frame2") "multipart: frame2")
    (assert (= (string (get frames 2)) "frame3") "multipart: frame3"))

  (zmq:close req)
  (zmq:close rep))

## ── Socket options ───────────────────────────────────────────────

(let [sock (zmq:socket ctx :req)]
  (zmq:set-option sock :linger 0)
  (assert (= (zmq:get-option sock :linger) 0) "linger set to 0")

  (zmq:set-option sock :sndhwm 500)
  (assert (= (zmq:get-option sock :sndhwm) 500) "sndhwm set to 500")

  (zmq:set-option sock :rcvhwm 500)
  (assert (= (zmq:get-option sock :rcvhwm) 500) "rcvhwm set to 500")

  (zmq:close sock))

## ── Bytes send/recv ──────────────────────────────────────────────

(let* [rep (zmq:socket ctx :rep)
       req (zmq:socket ctx :req)]
  (zmq:bind rep "inproc://test-bytes")
  (zmq:connect req "inproc://test-bytes")

  (zmq:send req (bytes 1 2 3 4 5))
  (let [msg (zmq:recv rep)]
    (assert (= msg (bytes 1 2 3 4 5)) "binary round-trip"))

  (zmq:send rep (bytes))
  (let [msg (zmq:recv req)]
    (assert (= msg (bytes)) "empty message round-trip"))

  (zmq:close req)
  (zmq:close rep))

## ── Cleanup ──────────────────────────────────────────────────────

(zmq:term ctx)

(println "all zmq tests passed.")
