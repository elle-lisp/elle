#!/usr/bin/env elle

# ZMQ — request/reply echo server and client
#
# Demonstrates:
#   lib/zmq.lisp          — FFI bindings to libzmq
#   zmq:context/socket    — ZMQ setup
#   zmq:bind/connect      — endpoint management
#   zmq:send/recv-string  — message passing
#   inproc:// transport   — in-process messaging (no network)

(let [([ok? _] (protect (ffi/native "libzmq.so")))]
  (unless ok?
    (println "SKIP: libzmq.so not found")
    (sys/exit 0)))

(def zmq (import-file "lib/zmq.lisp"))

(def ctx (zmq:context))

# ── REQ/REP echo ──────────────────────────────────────────────────

(println "req/rep echo:")

(let* [[rep (zmq:socket ctx :rep)]
       [req (zmq:socket ctx :req)]]
  (zmq:bind rep "inproc://echo")
  (zmq:connect req "inproc://echo")

  (each i in (range 5)
    (let [[msg (concat "hello " (string i))]]
      (zmq:send req msg)
      (let [[received (zmq:recv-string rep)]]
        (print "  server got: ") (println received)
        (zmq:send rep (concat "echo: " received)))
      (let [[reply (zmq:recv-string req)]]
        (print "  client got: ") (println reply))))

  (zmq:close req)
  (zmq:close rep))

# ── PUSH/PULL pipeline ───────────────────────────────────────────

(println "")
(println "push/pull pipeline:")

(let* [[pull (zmq:socket ctx :pull)]
       [push (zmq:socket ctx :push)]]
  (zmq:bind pull "inproc://pipeline")
  (zmq:connect push "inproc://pipeline")

  (each i in (range 3)
    (zmq:send push (concat "task " (string i))))

  (each _ in (range 3)
    (let [[msg (zmq:recv-string pull)]]
      (print "  worker got: ") (println msg)))

  (zmq:close push)
  (zmq:close pull))

# ── Multipart messages ───────────────────────────────────────────

(println "")
(println "multipart:")

(let* [[rep (zmq:socket ctx :rep)]
       [req (zmq:socket ctx :req)]]
  (zmq:bind rep "inproc://multi")
  (zmq:connect req "inproc://multi")

  (zmq:send-multipart req ["identity" "header" "body"])
  (let [[frames (zmq:recv-multipart rep)]]
    (print "  received ") (print (length frames)) (println " frames:")
    (each frame in frames
      (print "    ") (println (string frame))))

  (zmq:close req)
  (zmq:close rep))

(zmq:term ctx)

(println "")
(println "all zmq examples passed.")
