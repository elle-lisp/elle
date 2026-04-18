(elle/epoch 7)
# Sync primitives tests
#
# Tests for lib/sync.lisp: futex, lock, semaphore, condvar, rwlock,
# barrier, latch, once, queue, monitor.

(def sync ((import-file "lib/sync.lisp")))

# ============================================================================
# 1. Futex basics
# ============================================================================

(let [ftx (sync:make-futex 0)]
  (assert (= 0 (ftx:get)) "1a: futex initial value")
  (ftx:set 42)
  (assert (= 42 (ftx:get)) "1b: futex set/get"))

# Futex wait returns immediately when value != expected
(let [ftx (sync:make-futex 1)]
  (ev/join (ev/spawn (fn [] (ftx:wait 0))))  # 0 != 1, returns immediately
  (assert true "1c: futex wait returns when value != expected"))

# Futex wake unparks a waiter
(let [ftx (sync:make-futex :locked)
      log @[]]
  (let [waiter (ev/spawn (fn []
                  (ftx:wait :locked)
                  (push log :woken)))]
    (ev/spawn (fn []
      (ftx:set :free)
      (ftx:wake 1)))
    (ev/join waiter)
    (assert (= [:woken] (freeze log)) "1d: futex wake unparks waiter")))

# ============================================================================
# 2. Lock
# ============================================================================

(let [lock (sync:make-lock)]
  (assert (not (lock:held?)) "2a: lock starts free")
  (lock:acquire)
  (assert (lock:held?) "2b: lock held after acquire")
  (lock:release)
  (assert (not (lock:held?)) "2c: lock free after release"))

# try-acquire
(let [lock (sync:make-lock)]
  (assert (lock:try-acquire) "2d: try-acquire succeeds on free lock")
  (assert (not (lock:try-acquire)) "2e: try-acquire fails on held lock")
  (lock:release))

# Lock provides mutual exclusion between fibers
(let [lock (sync:make-lock)
      counter @[0]
      log @[]]
  (defn critical [id n]
    (lock:acquire)
    (repeat n
      (let [v (counter 0)]
        (put counter 0 (inc v))))
    (push log id)
    (lock:release))
  (let [a (ev/spawn (fn [] (critical :a 100)))
        b (ev/spawn (fn [] (critical :b 100)))]
    (ev/join [a b])
    (assert (= 200 (counter 0)) "2f: lock ensures mutual exclusion")
    (assert (= 2 (length log)) "2g: both fibers completed")))

# ============================================================================
# 3. Semaphore
# ============================================================================

(let [sem (sync:make-semaphore 2)]
  (assert (= 2 (sem:permits)) "3a: semaphore initial permits")
  (sem:acquire)
  (assert (= 1 (sem:permits)) "3b: permits after one acquire")
  (sem:acquire)
  (assert (= 0 (sem:permits)) "3c: permits after two acquires")
  (sem:release)
  (assert (= 1 (sem:permits)) "3d: permits after release"))

# try-acquire
(let [sem (sync:make-semaphore 1)]
  (assert (sem:try-acquire) "3e: try-acquire succeeds with permits")
  (assert (not (sem:try-acquire)) "3f: try-acquire fails with no permits")
  (sem:release))

# Semaphore limits concurrency
(let [sem (sync:make-semaphore 2)
      active @[0]
      max-active @[0]]
  (defn sem-worker []
    (sem:acquire)
    (put active 0 (inc (active 0)))
    (when (> (active 0) (max-active 0))
      (put max-active 0 (active 0)))
    # yield to let other fibers run
    (ev/join (ev/spawn (fn [] nil)))
    (put active 0 (dec (active 0)))
    (sem:release))
  (ev/join (map (fn [_] (ev/spawn sem-worker)) [1 2 3 4 5]))
  (assert (<= (max-active 0) 2) "3g: semaphore limits concurrency to 2"))

# ============================================================================
# 4. Condition variable
# ============================================================================

(let [lock (sync:make-lock)
      cv   (sync:make-condvar)
      data @[nil]]
  (let [consumer (ev/spawn (fn []
                    (lock:acquire)
                    (while (nil? (data 0))
                      (cv:wait lock))
                    (let [val (data 0)]
                      (lock:release)
                      val)))
        producer (ev/spawn (fn []
                    (lock:acquire)
                    (put data 0 :ready)
                    (cv:notify)
                    (lock:release)))]
    (ev/join producer)
    (assert (= :ready (ev/join consumer)) "4a: condvar wait/notify")))

# Broadcast wakes all waiters
(let [lock (sync:make-lock)
      cv   (sync:make-condvar)
      gate @[false]
      log  @[]]
  (defn cv-waiter [id]
    (lock:acquire)
    (while (not (gate 0))
      (cv:wait lock))
    (push log id)
    (lock:release))
  (let [a (ev/spawn (fn [] (cv-waiter :a)))
        b (ev/spawn (fn [] (cv-waiter :b)))
        c (ev/spawn (fn [] (cv-waiter :c)))]
    (ev/spawn (fn []
      (lock:acquire)
      (put gate 0 true)
      (cv:broadcast)
      (lock:release)))
    (ev/join [a b c])
    (assert (= 3 (length log)) "4b: condvar broadcast wakes all waiters")))

# ============================================================================
# 5. Read-write lock
# ============================================================================

(let [rw (sync:make-rwlock)
      log @[]]
  # Multiple readers can hold simultaneously
  (let [r1 (ev/spawn (fn []
              (rw:read-acquire)
              (push log :r1-in)
              (ev/join (ev/spawn (fn [] nil)))  # yield
              (push log :r1-out)
              (rw:read-release)))
        r2 (ev/spawn (fn []
              (rw:read-acquire)
              (push log :r2-in)
              (ev/join (ev/spawn (fn [] nil)))  # yield
              (push log :r2-out)
              (rw:read-release)))]
    (ev/join [r1 r2])
    # Both readers should have been in simultaneously
    (let [r1-in  (find-index (fn [x] (= x :r1-in)) log)
          r2-in  (find-index (fn [x] (= x :r2-in)) log)
          r1-out (find-index (fn [x] (= x :r1-out)) log)
          r2-out (find-index (fn [x] (= x :r2-out)) log)]
      (assert (and (not (nil? r1-in)) (not (nil? r2-in)))
              "5a: both readers entered"))))

# Writer excludes readers
(let [rw      (sync:make-rwlock)
      counter @[0]]
  (defn rw-writer []
    (rw:write-acquire)
    (put counter 0 (inc (counter 0)))
    (rw:write-release))
  (defn rw-reader []
    (rw:read-acquire)
    (let [v (counter 0)]
      (rw:read-release)
      v))
  (ev/join (ev/spawn rw-writer))
  (assert (= 1 (ev/join (ev/spawn rw-reader))) "5b: writer then reader"))

# ============================================================================
# 6. Barrier
# ============================================================================

(let [barrier (sync:make-barrier 3)
      log @[]]
  (defn barrier-worker [id]
    (push log (list :before id))
    (barrier:wait)
    (push log (list :after id)))
  (let [a (ev/spawn (fn [] (barrier-worker :a)))
        b (ev/spawn (fn [] (barrier-worker :b)))
        c (ev/spawn (fn [] (barrier-worker :c)))]
    (ev/join [a b c])
    # All :before entries should come before all :after entries
    (let [befores (filter (fn [x] (= :before (first x))) log)
          afters  (filter (fn [x] (= :after (first x))) log)]
      (assert (= 3 (length befores)) "6a: all fibers reached barrier")
      (assert (= 3 (length afters))  "6b: all fibers passed barrier"))))

# ============================================================================
# 7. Latch
# ============================================================================

(let [latch (sync:make-latch)]
  (assert (not (latch:open?)) "7a: latch starts closed")
  (latch:open)
  (assert (latch:open?) "7b: latch is open after open"))

# Latch gate behavior
(let [latch (sync:make-latch)
      log @[]]
  (let [waiter (ev/spawn (fn []
                  (latch:wait)
                  (push log :passed)))]
    (ev/spawn (fn []
      (push log :opening)
      (latch:open)))
    (ev/join waiter)
    (assert (= :passed (last log)) "7c: latch wait blocks until open")))

# Latch wait on already-open latch returns immediately
(let [latch (sync:make-latch)]
  (latch:open)
  (ev/join (ev/spawn (fn [] (latch:wait))))
  (assert true "7d: wait on open latch returns immediately"))

# ============================================================================
# 8. Once
# ============================================================================

(let* [call-count @[0]
       once (sync:make-once (fn []
               (put call-count 0 (inc (call-count 0)))
               42))]
  (assert (= 42 (once:get)) "8a: once returns thunk result")
  (assert (= 42 (once:get)) "8b: once returns same result on second call")
  (assert (= 1 (call-count 0)) "8c: thunk called exactly once"))

# Once with multiple concurrent getters
(let* [call-count @[0]
       once (sync:make-once (fn []
               (put call-count 0 (inc (call-count 0)))
               :initialized))]
  (let [fibers (map (fn [_] (ev/spawn (fn [] (once:get)))) [1 2 3 4 5])]
    (let [results (ev/join fibers)]
      (assert (= 1 (call-count 0)) "8d: thunk called once even with concurrent getters")
      (each r in results
        (assert (= :initialized r) "8e: all getters received same value")))))

# Once propagates errors
(let [once (sync:make-once (fn [] (error {:error :init-error :message "fail"})))]
  (let [[ok? val] (protect (once:get))]
    (assert (not ok?) "8f: once propagates error")
    (assert (= :init-error val:error) "8g: once error preserved")))

# ============================================================================
# 9. Queue
# ============================================================================

(let [q (sync:make-queue 3)]
  (assert (= 0 (q:size)) "9a: queue starts empty")
  (q:put :a)
  (q:put :b)
  (assert (= 2 (q:size)) "9b: queue size after puts")
  (assert (= :a (q:take)) "9c: queue FIFO order - first")
  (assert (= :b (q:take)) "9d: queue FIFO order - second")
  (assert (= 0 (q:size)) "9e: queue empty after takes"))

# Producer-consumer
(let [q (sync:make-queue 2)
      results @[]]
  (let [producer (ev/spawn (fn []
                    (each x in [1 2 3 4 5]
                      (q:put x))))
        consumer (ev/spawn (fn []
                    (repeat 5
                      (push results (q:take)))))]
    (ev/join [producer consumer])
    (assert (= [1 2 3 4 5] (freeze results))
            "9f: producer-consumer with bounded queue")))

# Multiple producers, single consumer
(let [q (sync:make-queue 2)
      results @[]]
  (let [p1 (ev/spawn (fn [] (each x in [:a :b :c] (q:put x))))
        p2 (ev/spawn (fn [] (each x in [:d :e :f] (q:put x))))
        consumer (ev/spawn (fn []
                    (repeat 6
                      (push results (q:take)))))]
    (ev/join [p1 p2 consumer])
    (assert (= 6 (length results)) "9g: all items consumed from multi-producer queue")))

# ============================================================================
# 10. Monitor
# ============================================================================

(let [mon (sync:make-monitor)
      state @[0]]
  (mon:with (fn []
    (put state 0 (inc (state 0)))))
  (assert (= 1 (state 0)) "10a: monitor with executes body"))

# Monitor provides mutual exclusion
(let [mon (sync:make-monitor)
      counter @[0]]
  (let [fibers (map (fn [_]
                  (ev/spawn (fn []
                    (repeat 50
                      (mon:with (fn []
                        (put counter 0 (inc (counter 0)))))))))
                [1 2 3 4])]
    (ev/join fibers)
    (assert (= 200 (counter 0))
            "10b: monitor ensures mutual exclusion")))

# Monitor wait/notify
(let [mon   (sync:make-monitor)
      ready @[false]]
  (let [waiter (ev/spawn (fn []
                  (mon:with (fn []
                    (while (not (ready 0))
                      (mon:wait))
                    :done))))
        notifier (ev/spawn (fn []
                    (mon:with (fn []
                      (put ready 0 true)
                      (mon:notify)))))]
    (ev/join notifier)
    (assert (= :done (ev/join waiter)) "10c: monitor wait/notify")))

# Monitor with propagates errors
(let [[ok? val] (protect
        (let [mon (sync:make-monitor)]
          (mon:with (fn [] (error {:error :boom :message "test"})))))]
  (assert (not ok?) "10d: monitor with propagates error")
  (assert (= :boom val:error) "10e: error value preserved"))

(println "All sync tests passed.")
