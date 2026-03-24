#!/usr/bin/env elle

# Processes — Erlang-style concurrent task processing
#
# Demonstrates the lib/process module through six scenarios:
#   1. Ping-pong            — basic send/recv
#   2. Supervised workers   — monitors, crash restart, named processes
#   3. Preemptive ring      — fuel-based fair scheduling
#   4. Selective receive    — recv-match, timers, timeout
#   5. Key-value service    — named process + process dictionary
#   6. I/O inside processes — println doesn't block sibling processes

(def process ((import-file "lib/process.lisp")))

# Share the ev/run backend so all process schedulers use one event loop
(def backend (*io-backend*))


# ========================================
# 1. Ping-pong
# ========================================

(process:start (fn []
  (let* ([me (process:self)]
         [peer (process:spawn (fn []
                 (match (process:recv)
                   ([from :ping] (process:send from :pong))
                   (_ nil))))])
    (process:send peer [me :ping])
    (let ([reply (process:recv)])
      (println (string "  ping-pong: " reply))
      (assert (= reply :pong) "ping-pong"))))
  :backend backend)


# ========================================
# 2. Supervised workers
# ========================================
# Three workers square numbers. Worker 2 crashes on job 4. The supervisor
# detects the crash via a monitor, restarts the worker, and re-dispatches.

(defn run-supervised []
  (process:trap-exit true)
  (let ([me (process:self)])

    # Collector: accumulates results, sends them back when done
    (process:spawn (fn []
      (process:register :collector)
      (var results @[])
      (var remaining 6)
      (while (> remaining 0)
        (match (process:recv)
          ([:result job-id value]
            (push results [job-id value])
            (assign remaining (- remaining 1)))
          (_ nil)))
      (sort results)
      (process:send me [:done (freeze results)])))

    # Worker factory
    (defn make-worker [id]
      (fn []
        (forever
          (match (process:recv)
            ([:job job-id payload]
              # Worker 2 crashes on job 4
              (when (and (= id 2) (= job-id 4))
                (error {:error :crash :message "boom"}))
              (process:send-named :collector [:result job-id (* payload payload)]))
            ([:stop] (break))
            (_ nil)))))

    # Spawn 3 workers with monitors
    (var pids @[(get (process:spawn-monitor (make-worker 1)) 0)
                (get (process:spawn-monitor (make-worker 2)) 0)
                (get (process:spawn-monitor (make-worker 3)) 0)])

    # Dispatch 6 jobs round-robin
    (var i 0)
    (each [jid payload] in [[1 10] [2 20] [3 30] [4 40] [5 50] [6 60]]
      (process:send (get pids (% i 3)) [:job jid payload])
      (assign i (+ i 1)))

    # Wait for results, handling crashes
    (var result nil)
    (while (nil? result)
      (match (process:recv)
        ([:DOWN _ref dead-pid _reason]
          (println "  supervisor: worker crashed, restarting")
          (let ([new-pid (get (process:spawn-monitor (make-worker 2)) 0)])
            (var j 0)
            (while (< j (length pids))
              (when (= (get pids j) dead-pid) (put pids j new-pid))
              (assign j (+ j 1)))
            (process:send new-pid [:job 4 40])))
        ([:done results] (assign result results))
        (_ nil)))

    (each pid in pids (process:send pid [:stop]))

    (println (string "  worker pool: " result))
    (assert (= (length result) 6) "all 6 results")
    (assert (= (get (get result 0) 1) 100) "10² = 100")
    (assert (= (get (get result 5) 1) 3600) "60² = 3600")))

(process:start run-supervised :backend backend)


# ========================================
# 3. Preemptive ring
# ========================================
# A message passes through three forwarders while a CPU-hog runs alongside.

(defn run-ring []
  (let ([me (process:self)])
    (defn make-node [next]
      (fn [] (process:send next (+ (process:recv) 1))))

    (let* ([n3 (process:spawn (make-node me))]
           [n2 (process:spawn (make-node n3))]
           [n1 (process:spawn (make-node n2))]
           [hog (process:spawn (fn []
                  (letrec ([spin (fn [n] (spin (+ n 1)))]) (spin 0))))])
      (process:send n1 0)
      (let ([val (process:recv)])
        (println (string "  ring: 0 → " val " (with cpu hog)"))
        (assert (= val 3) "ring increments 3 times")
        (process:exit hog :kill)))))

(process:start run-ring :fuel 200 :backend backend)


# ========================================
# 4. Selective receive and timers
# ========================================

(defn run-selective []
  (let ([me (process:self)])
    (process:send me [:low 1])
    (process:send me [:high 99])
    (process:send me [:low 2])

    # Grab high-priority first
    (let ([urgent (process:recv-match (fn [m] (= (get m 0) :high)))])
      (println (string "  selective recv: " urgent))
      (assert (= (get urgent 1) 99) "high-priority first"))

    # Remaining in order
    (let* ([a (process:recv)] [b (process:recv)])
      (assert (= (get a 1) 1) "low 1")
      (assert (= (get b 1) 2) "low 2"))

    # Timer
    (process:send-after 3 me :alarm)
    (let ([msg (process:recv)])
      (println (string "  timer: " msg))
      (assert (= msg :alarm) "timer fires"))

    # Timeout
    (let ([msg (process:recv-timeout 1)])
      (println (string "  timeout: " msg))
      (assert (= msg :timeout) "times out"))))

(process:start run-selective :backend backend)


# ========================================
# 5. Key-value service
# ========================================

(defn run-kv []
  (let ([me (process:self)])

    # Server: uses process dictionary as storage
    (process:spawn (fn []
      (process:register :kv)
      (forever
        (match (process:recv)
          ([:put from key val]
            (process:put-dict key val)
            (process:send from :ok))
          ([:get from key]
            (process:send from (process:get-dict key)))
          ([:stop] (break))
          (_ nil)))))

    # Client
    (process:send-named :kv [:put me :name "elle"])
    (process:recv)
    (process:send-named :kv [:put me :version 5])
    (process:recv)

    (process:send-named :kv [:get me :name])
    (let ([v (process:recv)])
      (println (string "  kv :name → " v))
      (assert (= v "elle") "kv: name"))

    (process:send-named :kv [:get me :version])
    (let ([v (process:recv)])
      (println (string "  kv :version → " v))
      (assert (= v 5) "kv: version"))

    (process:send-named :kv [:get me :missing])
    (let ([v (process:recv)])
      (println (string "  kv :missing → " v))
      (assert (nil? v) "kv: nil for missing"))

    (process:send-named :kv [:stop])))

(process:start run-kv :backend backend)


# ========================================
# 6. I/O inside processes
# ========================================
# Processes can freely use println — the scheduler submits I/O to the
# async backend and parks the process, so other processes keep running.

(process:start (fn []
  (let ([me (process:self)])
    (process:spawn (fn []
      (println "  io-process: hello from process A")
      (process:send me :a-done)))
    (process:spawn (fn []
      (println "  io-process: hello from process B")
      (process:send me :b-done)))
    (var remaining 2)
    (while (> remaining 0)
      (match (process:recv)
        (:a-done (assign remaining (- remaining 1)))
        (:b-done (assign remaining (- remaining 1)))
        (_ nil)))
    (println "  io-process: both completed")))
  :backend backend)


(println "")
(println "all processes passed.")
