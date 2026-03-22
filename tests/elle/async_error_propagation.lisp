# Async error propagation tests
#
# These tests document the expected behavior of error propagation through
# ev/gather, the async scheduler, and stream combinators. They serve as
# regression tests for a multi-session debugging effort.

# === Helpers ===

(defn make-range [n]
  (coro/new (fn []
    (var i 0)
    (while (< i n)
      (yield i)
      (assign i (+ i 1))))))

# === 1. Synchronous coroutine errors propagate through stream/collect ===

(let (([ok? val] (protect ((fn []
    (let [[co (coro/new (fn [] (error "boom")))]]
      (coro/resume co)))))))
  (assert (not ok?) "1a: error from coroutine propagates through coro/resume"))

(let (([ok? val] (protect ((fn []
    (let [[co (coro/new (fn []
            (yield 1)
            (yield 2)
            (error "boom")
            (yield 3)))]]
      (stream/collect co)))))))
  (assert (not ok?) "1b: stream/collect propagates coroutine error"))

# === 2. stream/map error propagation (no async, no scheduler) ===

(let (([ok? _] (protect ((fn []
    (stream/collect
      (stream/map
        (fn [v] (when (= v 1) (error {:error :test-error :message "stop at 1"})) v)
        (make-range 3))))))))
  (assert (not ok?) "2: stream/map transform error propagates through collect"))

# === 3. Basic async I/O through ev/gather (no error) ===

(spit "/tmp/elle-async-err-test-3" "line1\nline2\nline3\n")
(let [[result (ev/gather (fn []
    (let [[p (port/open "/tmp/elle-async-err-test-3" :read)]]
      (stream/collect (port/lines p)))))]]
  (assert (= (length result) 3) "3: ev/gather + port/lines collects 3 lines"))

# === 4. Closed port error propagates through ev/gather ===
#
# This is the target bug: reading from a closed port inside ev/gather
# should propagate the error to the caller via protect.
# STATUS: this test documents the DESIRED behavior. If it fails, the
# error propagation path (io/submit failure → fiber/abort → ev/gather)
# is broken.

(spit "/tmp/elle-async-err-test-4" "some data")
(let (([ok? val] (protect ((fn []
    (ev/gather (fn []
      (let [[p (port/open "/tmp/elle-async-err-test-4" :read)]]
        (port/close p)
        (stream/collect (port/lines p))))))))))
  (assert (not ok?) "4: closed port error propagates through ev/gather"))

# === 5. Multiple fibers in ev/gather, one errors ===

(spit "/tmp/elle-async-err-test-5" "data")
(let (([ok? val] (protect ((fn []
    (ev/gather
      (fn [] 42)
      (fn []
        (let [[p (port/open "/tmp/elle-async-err-test-5" :read)]]
          (port/close p)
          (stream/collect (port/lines p))))))))))
  (assert (not ok?) "5: one erroring fiber in ev/gather propagates"))

(println "all async error propagation tests passed")
