#!/usr/bin/env elle

# Channels — inter-fiber message passing with crossbeam-channel
#
# Demonstrates:
#   chan/new              — create bounded and unbounded channels
#   chan/send, chan/recv   — non-blocking send/receive with status tuples
#   chan/clone            — multiple senders feeding one receiver
#   chan/close            — explicit disconnect
#   chan/select           — multiplexed wait on multiple receivers
#   Keyword messages      — sending :ok, :empty etc. as values (no ambiguity)

(import-file "./examples/assertions.lisp")


# ========================================
# 1. Unbounded channel basics
# ========================================

# chan/new returns [sender receiver] as a tuple.
# chan/send is non-blocking: returns [:ok], [:full], or [:disconnected].
# chan/recv is non-blocking: returns [:ok msg], [:empty], or [:disconnected].
(let* (([s r] (chan/new))
       (send-result (chan/send s 42))
       (recv-result (chan/recv r)))
  (display "  send result: ") (print send-result)
  (display "  recv result: ") (print recv-result)
  (assert-eq (get send-result 0) :ok "unbounded send returns :ok")
  (assert-eq (get recv-result 0) :ok "recv status is :ok")
  (assert-eq (get recv-result 1) 42 "recv message is 42"))


# ========================================
# 2. Bounded channel with backpressure
# ========================================

(let* (([s r] (chan/new 1))
       (first (chan/send s "hello"))
       (second (chan/send s "world")))
  (display "  bounded(1) first send: ") (print first)
  (display "  bounded(1) second send: ") (print second)
  (assert-eq (get first 0) :ok "first send fits")
  (assert-eq (get second 0) :full "second send is full"))


# ========================================
# 3. Empty and disconnected states
# ========================================

(let (([s r] (chan/new)))
  (let ((empty-result (chan/recv r)))
    (display "  recv from empty: ") (print empty-result)
    (assert-eq (get empty-result 0) :empty "empty channel returns :empty"))

  (chan/close s)
  (let ((disc-result (chan/recv r)))
    (display "  recv after close: ") (print disc-result)
    (assert-eq (get disc-result 0) :disconnected "closed sender means :disconnected")))


# ========================================
# 4. Keyword values are not confused with status
# ========================================

# This is the critical test: sending :empty, :ok, :full, :disconnected
# as message values. The tuple protocol keeps status and message separate.
(let (([s r] (chan/new)))
  (chan/send s :empty)
  (chan/send s :ok)
  (chan/send s :disconnected)
  (let* ((r1 (chan/recv r))
         (r2 (chan/recv r))
         (r3 (chan/recv r)))
    (display "  sent :empty, got: ") (print r1)
    (display "  sent :ok, got: ") (print r2)
    (display "  sent :disconnected, got: ") (print r3)
    (assert-eq (get r1 0) :ok "status is :ok")
    (assert-eq (get r1 1) :empty "message is :empty (not confused with status)")
    (assert-eq (get r2 1) :ok "message is :ok (not confused with status)")
    (assert-eq (get r3 1) :disconnected "message is :disconnected (not confused with status)")))


# ========================================
# 5. Multiple senders via chan/clone
# ========================================

(let* (([s r] (chan/new))
       (s2 (chan/clone s)))
  (chan/send s "from-original")
  (chan/send s2 "from-clone")
  (let* ((r1 (chan/recv r))
         (r2 (chan/recv r)))
    (display "  original sender: ") (print (get r1 1))
    (display "  cloned sender: ") (print (get r2 1))
    (assert-eq (get r1 1) "from-original" "original sender message")
    (assert-eq (get r2 1) "from-clone" "cloned sender message")))


# ========================================
# 6. chan/select — multiplexed receive
# ========================================

(let* (([s1 r1] (chan/new))
       ([s2 r2] (chan/new)))
  (chan/send s2 "second-wins")
  # r1 is empty, r2 has a message — select should pick r2 (index 1).
  (let ((result (chan/select @[r1 r2] 1000)))
    (display "  select picked index ") (display (get result 0))
    (display " with message: ") (print (get result 1))
    (assert-eq (get result 0) 1 "select returns index of ready receiver")
    (assert-eq (get result 1) "second-wins" "select returns the message")))

# Timeout when nothing is ready.
(let (([s r] (chan/new)))
  (let ((result (chan/select @[r] 10)))
    (display "  select timeout: ") (print result)
    (assert-eq (get result 0) :timeout "select times out on empty channel")))


(print "")
(print "all channel tests passed.")
