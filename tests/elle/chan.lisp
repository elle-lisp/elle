# Channel Tests
#
# Tests for Elle's channel primitives (chan, chan/send, chan/recv, chan/select, etc.)
# Covers unbounded and bounded channels, send/recv operations, disconnection, cloning,
# keyword message handling, and select with timeout.

(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "./examples/assertions.lisp")))

# ============================================================================
# Test 1: chan unbounded
# ============================================================================

(let (([s r] (chan)))
  (assert-true (array? [s r]) "chan should return an array"))

# ============================================================================
# Test 2: chan bounded
# ============================================================================

(let (([s r] (chan 10)))
  (assert-true (array? [s r]) "chan with capacity should return an array"))

# ============================================================================
# Test 3: chan/send ok
# ============================================================================

(let (([s r] (chan)))
  (let ((result (chan/send s 42)))
    (assert-true (array? result) "chan/send should return an array")
    (assert-eq (length result) 1 "send result should have 1 element")
    (assert-eq (get result 0) :ok "send result should be [:ok]")))

# ============================================================================
# Test 4: chan/recv ok
# ============================================================================

(let (([s r] (chan)))
  (chan/send s 42)
  (let ((result (chan/recv r)))
    (assert-true (array? result) "chan/recv should return an array")
    (assert-eq (length result) 2 "recv result should have 2 elements")
    (assert-eq (get result 0) :ok "recv status should be :ok")
    (assert-eq (get result 1) 42 "recv message should be 42")))

# ============================================================================
# Test 5: chan/recv empty
# ============================================================================

(let (([s r] (chan)))
  (let ((result (chan/recv r)))
    (assert-true (array? result) "chan/recv should return an array")
    (assert-eq (length result) 1 "empty recv result should have 1 element")
    (assert-eq (get result 0) :empty "empty recv should return [:empty]")))

# ============================================================================
# Test 6: chan/send full
# ============================================================================

(let (([s r] (chan 1)))
  (chan/send s 1)
  (let ((result (chan/send s 2)))
    (assert-true (array? result) "chan/send to full should return an array")
    (assert-eq (length result) 1 "full send result should have 1 element")
    (assert-eq (get result 0) :full "full send should return [:full]")))

# ============================================================================
# Test 7: disconnected send
# ============================================================================

(let (([s r] (chan)))
  (chan/close-recv r)
  (let ((result (chan/send s 42)))
    (assert-true (array? result) "chan/send to closed receiver should return an array")
    (assert-eq (length result) 1 "disconnected send result should have 1 element")
    (assert-eq (get result 0) :disconnected "disconnected send should return [:disconnected]")))

# ============================================================================
# Test 8: disconnected recv
# ============================================================================

(let (([s r] (chan)))
  (chan/close s)
  (let ((result (chan/recv r)))
    (assert-true (array? result) "chan/recv from closed sender should return an array")
    (assert-eq (length result) 1 "disconnected recv result should have 1 element")
    (assert-eq (get result 0) :disconnected "disconnected recv should return [:disconnected]")))

# ============================================================================
# Test 9: chan/clone
# ============================================================================

(let* (([s r] (chan))
       (s2 (chan/clone s)))
  (chan/send s 1)
  (chan/send s2 2)
  (let* ((r1 (chan/recv r))
         (r2 (chan/recv r)))
    (assert-eq (length r1) 2 "r1 should have 2 elements")
    (assert-eq (get r1 0) :ok "r1 status should be :ok")
    (assert-eq (get r1 1) 1 "r1 message should be 1")
    (assert-eq (length r2) 2 "r2 should have 2 elements")
    (assert-eq (get r2 0) :ok "r2 status should be :ok")
    (assert-eq (get r2 1) 2 "r2 message should be 2")))

# ============================================================================
# Test 10: keyword messages
# ============================================================================

(let (([s r] (chan)))
  (chan/send s :empty)
  (chan/send s :full)
  (chan/send s :ok)
  (chan/send s :disconnected)
  (let* ((r1 (chan/recv r))
         (r2 (chan/recv r))
         (r3 (chan/recv r))
         (r4 (chan/recv r)))
    # r1 should be [:ok :empty]
    (assert-eq (length r1) 2 "r1 should have 2 elements")
    (assert-eq (get r1 0) :ok "r1 status should be :ok")
    (assert-eq (get r1 1) :empty "r1 message should be :empty")
    # r2 should be [:ok :full]
    (assert-eq (length r2) 2 "r2 should have 2 elements")
    (assert-eq (get r2 0) :ok "r2 status should be :ok")
    (assert-eq (get r2 1) :full "r2 message should be :full")
    # r3 should be [:ok :ok]
    (assert-eq (length r3) 2 "r3 should have 2 elements")
    (assert-eq (get r3 0) :ok "r3 status should be :ok")
    (assert-eq (get r3 1) :ok "r3 message should be :ok")
    # r4 should be [:ok :disconnected]
    (assert-eq (length r4) 2 "r4 should have 2 elements")
    (assert-eq (get r4 0) :ok "r4 status should be :ok")
    (assert-eq (get r4 1) :disconnected "r4 message should be :disconnected")))

# ============================================================================
# Test 11: chan/select ready
# ============================================================================

(let (([s r] (chan)))
  (chan/send s 99)
  (let ((result (chan/select @[r] 1000)))
    (assert-true (array? result) "chan/select should return a tuple")
    (assert-eq (length result) 2 "select ready result should have 2 elements")
    (assert-eq (get result 0) 0 "select ready should return index 0")
    (assert-eq (get result 1) 99 "select ready should return message 99")))

# ============================================================================
# Test 12: chan/select timeout
# ============================================================================

(let (([s r] (chan)))
  (let ((result (chan/select @[r] 10)))
    (assert-true (array? result) "chan/select should return a tuple")
    (assert-eq (length result) 1 "select timeout result should have 1 element")
    (assert-eq (get result 0) :timeout "select timeout should return [:timeout]")))
