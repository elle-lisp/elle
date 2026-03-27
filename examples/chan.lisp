#!/usr/bin/env elle

# Channels — inter-fiber message passing with crossbeam-channel
#
# Demonstrates:
#   chan              — create bounded and unbounded channels
#   chan/send, chan/recv   — non-blocking send/receive with status tuples
#   chan/clone            — multiple senders feeding one receiver
#   chan/close            — explicit disconnect
#   chan/select           — multiplexed wait on multiple receivers
#   Keyword messages      — sending :ok, :empty etc. as values (no ambiguity)
## ── Unbounded channel basics ───────────────────────────────────────

# chan returns [sender receiver] as an array.
# chan/send is non-blocking: returns [:ok], [:full], or [:disconnected].
# chan/recv is non-blocking: returns [:ok msg], [:empty], or [:disconnected].
(let* (([s r] (chan))
       (send-result (chan/send s 42))
       (recv-result (chan/recv r)))
  (println "  send result: " send-result)
  (println "  recv result: " recv-result)
  (assert (= (get send-result 0) :ok) "unbounded send returns :ok")
  (assert (= (get recv-result 0) :ok) "recv status is :ok")
  (assert (= (get recv-result 1) 42) "recv message is 42"))
## ── Bounded channel with backpressure ──────────────────────────────

(let* (([s r] (chan 1))
       (first (chan/send s "hello"))
       (second (chan/send s "world")))
  (println "  bounded(1) first send: " first)
  (println "  bounded(1) second send: " second)
  (assert (= (get first 0) :ok) "first send fits")
  (assert (= (get second 0) :full) "second send is full"))
## ── Empty and disconnected states ──────────────────────────────────

(let (([s r] (chan)))
  (let ((empty-result (chan/recv r)))
    (println "  recv from empty: " empty-result)
    (assert (= (get empty-result 0) :empty) "empty channel returns :empty"))

  (chan/close s)
  (let ((disc-result (chan/recv r)))
    (println "  recv after close: " disc-result)
    (assert (= (get disc-result 0) :disconnected) "closed sender means :disconnected")))
## ── Keyword values are not confused with status ────────────────────

# This is the critical test: sending :empty, :ok, :full, :disconnected
# as message values. The array protocol keeps status and message separate.
(let (([s r] (chan)))
  (chan/send s :empty)
  (chan/send s :ok)
  (chan/send s :disconnected)
  (let* ((r1 (chan/recv r))
         (r2 (chan/recv r))
         (r3 (chan/recv r)))
    (println "  sent :empty, got: " r1)
    (println "  sent :ok, got: " r2)
    (println "  sent :disconnected, got: " r3)
    (assert (= (get r1 0) :ok) "status is :ok")
    (assert (= (get r1 1) :empty) "message is :empty (not confused with status)")
    (assert (= (get r2 1) :ok) "message is :ok (not confused with status)")
    (assert (= (get r3 1) :disconnected) "message is :disconnected (not confused with status)")))
## ── Multiple senders via chan/clone ────────────────────────────────

(let* (([s r] (chan))
       (s2 (chan/clone s)))
  (chan/send s "from-original")
  (chan/send s2 "from-clone")
  (let* ((r1 (chan/recv r))
         (r2 (chan/recv r)))
    (println "  original sender: " (get r1 1))
    (println "  cloned sender: " (get r2 1))
    (assert (= (get r1 1) "from-original") "original sender message")
    (assert (= (get r2 1) "from-clone") "cloned sender message")))
## ── chan/select — multiplexed receive ──────────────────────────────

(let* (([s1 r1] (chan))
       ([s2 r2] (chan)))
  (chan/send s2 "second-wins")
  # r1 is empty, r2 has a message — select should pick r2 (index 1).
  (let ((result (chan/select @[r1 r2] 1000)))
    (print "  select picked index ") (print (get result 0))
    (println " with message: " (get result 1))
    (assert (= (get result 0) 1) "select returns index of ready receiver")
    (assert (= (get result 1) "second-wins") "select returns the message")))

# Timeout when nothing is ready.
(let (([s r] (chan)))
  (let ((result (chan/select @[r] 10)))
    (println "  select timeout: " result)
    (assert (= (get result 0) :timeout) "select times out on empty channel")))


(println "")
(println "all channel tests passed.")
