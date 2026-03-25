# Async error propagation tests
#
# These tests document the expected behavior of error propagation through
# ev/join, the async scheduler, and stream combinators. They serve as
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

# === 3. Basic async I/O with ev/join (no error) ===

(spit "/tmp/elle-async-err-test-3" "line1\nline2\nline3\n")
(let [[result (ev/join (ev/spawn (fn []
    (let [[p (port/open "/tmp/elle-async-err-test-3" :read)]]
      (stream/collect (port/lines p))))))]]
  (assert (= (length result) 3) "3: ev/join + port/lines collects 3 lines"))

# === 4. Closed port: port/lines yields the io-error as a value ===

(spit "/tmp/elle-async-err-test-4" "some data")
(let (([ok? val] (ev/join-protected (ev/spawn (fn []
    (let [[p (port/open "/tmp/elle-async-err-test-4" :read)]]
      (port/close p)
      (stream/collect (port/lines p))))))))
  (assert ok? "4: stream/collect succeeds (error yielded as value, not signaled)")
  (assert (= (length val) 1) "4: one element collected")
  (assert (= (get (first val) :error) :io-error) "4: collected element is io-error"))

# === 5. Multiple fibers, one errors — ev/join-protected catches ===

(spit "/tmp/elle-async-err-test-5" "data")
(let ([f1 (ev/spawn (fn [] 42))]
      [f2 (ev/spawn (fn []
        (let [[p (port/open "/tmp/elle-async-err-test-5" :read)]]
          (port/close p)
          (stream/collect (port/lines p)))))])
  (let (([ok1? v1] (ev/join-protected f1))
        ([ok2? v2] (ev/join-protected f2)))
    (assert ok1? "5a: first fiber succeeds")
    (assert (= 42 v1) "5b: first fiber returns 42")
    (assert ok2? "5c: second fiber succeeds (io-error yielded as value)")
    (assert (= (get (first v2) :error) :io-error) "5d: collected io-error")))

(println "all async error propagation tests passed")
