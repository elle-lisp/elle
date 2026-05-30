(elle/epoch 10)
## lib/sync.lisp — Concurrency primitives built on futex (park/notify)
##
## Loaded via: (def sync ((import "std/sync")))
## Usage:      (def lock (sync:make-lock))
##             (lock:acquire)
##             (lock:release)
##
## All primitives are cooperative (fiber-level), not OS-level.
## They use ev/futex-wait and ev/futex-wake from stdlib.

## ── Layer 1: Futex ──────────────────────────────────────────────────

## Futex identity must be unique across the WHOLE process, not just
## within this module instance.  `(import ...)` returns a fresh module
## each call, so a module-local counter restarts at 0 in every importer
## — two independently-imported sync modules would then hand out
## colliding keys, and since the scheduler's park-queue is process-global
## (one key → one wait list), a wake on one futex would unpark a waiter
## on the other (which re-checks its unchanged value and re-parks),
## losing the intended wakeup.  `gensym` is a process-global primitive
## counter, so keys are globally unique regardless of how many times the
## module is imported.
(defn make-futex [initial]
  "Low-level futex. Wraps a box with park/notify."
  (let [key (gensym)
        bx (box initial)]
    {:wait (fn [expected]
             (while (= (unbox bx) expected) (ev/futex-wait key bx expected)))
     :wake (fn [count] (ev/futex-wake key count))
     :get (fn [] (unbox bx))
     :set (fn [v] (rebox bx v))
     :box bx}))

## ── Layer 2: Core primitives ────────────────────────────────────────

## ── Lock (mutual exclusion) ─────────────────────────────────────────

(defn make-lock []
  "Mutual exclusion lock. false = free, true = held."
  (let [ftx (make-futex false)]
    {:acquire (fn []
                (while true
                  (when (not (ftx:get))
                    (ftx:set true)
                    (break nil))
                  (ftx:wait true)))
     :release (fn []
                (ftx:set false)
                (ftx:wake 1)
                nil)
     :try-acquire (fn []
                    (if (not (ftx:get))
                      (begin
                        (ftx:set true)
                        true)
                      false))
     :held? (fn [] (ftx:get))}))

## ── Semaphore (counting permits) ─────────────────────────────────────

(defn make-semaphore [n]
  "Counting semaphore with n initial permits."
  (let [ftx (make-futex n)]
    {:acquire (fn []
                (while true
                  (let [p (ftx:get)]
                    (when (> p 0)
                      (ftx:set (dec p))
                      (break nil))
                    (ftx:wait p))))
     :release (fn []
                (ftx:set (inc (ftx:get)))
                (ftx:wake 1)
                nil)
     :try-acquire (fn []
                    (let [p (ftx:get)]
                      (if (> p 0)
                        (begin
                          (ftx:set (dec p))
                          true)
                        false)))
     :permits (fn [] (ftx:get))}))

## ── Condition variable ───────────────────────────────────────────────

(defn make-condvar []
  "Condition variable using a generation counter."
  (let [ftx (make-futex 0)]
    {:wait (fn [lock]
             (let [gen (ftx:get)]
               (lock:release)
               (ftx:wait gen)
               (lock:acquire))
             nil)
     :notify (fn []
               (ftx:set (inc (ftx:get)))
               (ftx:wake 1)
               nil)
     :broadcast (fn []
                  (ftx:set (inc (ftx:get)))
                  (ftx:wake 999999999)
                  nil)}))

## ── Layer 3: Composed primitives ────────────────────────────────────

## ── Read-write lock ─────────────────────────────────────────────────

(defn make-rwlock []
  "Read-write lock. Multiple readers OR one writer."
  (let [state @[0]  # positive = reader count, -1 = writer held
        ftx (make-futex 0)]
    {:read-acquire (fn []
                     (while true
                       (let [s (state 0)]
                         (when (>= s 0)
                           (put state 0 (inc s))
                           (break nil))
                         (ftx:wait (ftx:get)))))
     :read-release (fn []
                     (put state 0 (dec (state 0)))
                     (ftx:set (inc (ftx:get)))
                     (ftx:wake 999999999)
                     nil)
     :write-acquire (fn []
                      (while true
                        (let [s (state 0)]
                          (when (= s 0)
                            (put state 0 (- 0 1))
                            (break nil))
                          (ftx:wait (ftx:get)))))
     :write-release (fn []
                      (put state 0 0)
                      (ftx:set (inc (ftx:get)))
                      (ftx:wake 999999999)
                      nil)}))

## ── Barrier ──────────────────────────────────────────────────────────

(defn make-barrier [n]
  "Barrier for N fibers. All must call :wait before any proceed."
  (let [ftx (make-futex 0)]
    {:wait (fn []
             (let [count (inc (ftx:get))]
               (ftx:set count)
               (if (= count n)
                 (begin
                   (ftx:wake 999999999)
                   nil)
                 (begin
                   (ftx:wait count)
                   nil))))}))

## ── Latch (one-shot gate) ────────────────────────────────────────────

(defn make-latch []
  "One-shot gate. Once opened, stays open."
  (let [ftx (make-futex false)]
    {:wait (fn []
             (ftx:wait false)
             nil)
     :open (fn []
             (ftx:set true)
             (ftx:wake 999999999)
             nil)
     :open? (fn [] (ftx:get))}))

## ── Once (lazy one-time init) ────────────────────────────────────────

(defn make-once [thunk]
  "Run thunk exactly once. All callers of :get receive the cached result."
  (let [ftx (make-futex :pending)
        result @[nil]]
    {:get (fn []
            (let [s (ftx:get)]
              (cond
                (= s :done) (result 0)
                (= s :running) (begin
                                 (ftx:wait :running)
                                 (result 0))
                true
                  (begin
                    (ftx:set :running)
                    (let [[ok? val] (protect (thunk))]
                      (put result 0 val)
                      (ftx:set :done)
                      (ftx:wake 999999999)
                      (if ok? val (error val)))))))}))

## ── Blocking queue ───────────────────────────────────────────────────

(defn make-queue [capacity]
  "Bounded blocking FIFO queue with cancellation via :close."
  (let [lock (make-lock)
        not-full (make-condvar)
        not-empty (make-condvar)
        buf @[]
        cap capacity
        @closed false]
    {:put (fn [val]
            (lock:acquire)
            (while (and (not closed) (>= (length buf) cap)) (not-full:wait lock))
            (if closed
              (begin
                (lock:release)
                nil)
              (begin
                (push buf val)
                (not-empty:notify)
                (lock:release)
                nil)))
     :take (fn []
             (lock:acquire)
             (while (and (not closed) (= (length buf) 0)) (not-empty:wait lock))
             (if (= (length buf) 0)
               (begin
                 (lock:release)
                 nil)
               (let [val (buf 0)]
                 (remove buf 0)
                 (not-full:notify)
                 (lock:release)
                 val)))
     :close (fn []
              (lock:acquire)
              (assign closed true)
              (not-full:broadcast)
              (not-empty:broadcast)
              (lock:release)
              nil)
     :closed? (fn [] closed)
     :size (fn [] (length buf))}))

## ── Monitor ──────────────────────────────────────────────────────────

(defn make-monitor []
  "Bundled lock + condvar for synchronized access to shared state."
  (let [lock (make-lock)
        cv (make-condvar)]
    {:with (fn [body-fn]
             (lock:acquire)
             (let [[ok? val] (protect (body-fn))]
               (lock:release)
               (if ok? val (error val))))
     :wait (fn [] (cv:wait lock))
     :notify (fn [] (cv:notify))
     :broadcast (fn [] (cv:broadcast))}))

## ── Export ───────────────────────────────────────────────────────────

(fn []
  {:make-futex make-futex
   :make-lock make-lock
   :make-semaphore make-semaphore
   :make-condvar make-condvar
   :make-rwlock make-rwlock
   :make-barrier make-barrier
   :make-latch make-latch
   :make-once make-once
   :make-queue make-queue
   :make-monitor make-monitor})
