(elle/epoch 9)
## tests/elle/process.lisp — Tests for lib/process.lisp
##
## Run: ./target/debug/elle tests/elle/process.lisp

(def process ((import-file "lib/process.lisp")))
(def backend (*io-backend*))

# Wrap process:start to always share the scheduler backend
(def process:start-raw process:start)
(defn process:start [init &named fuel]
  (process:start-raw init :fuel fuel :backend backend))

# ============================================================================
# 1. Ping-pong
# ============================================================================

(process:start (fn ()
                 (let [me (process:self)
                       ponger (process:spawn (fn ()
                         (let [msg (process:recv)]
                           (match msg
                             [from :ping] (process:send from :pong)
                             _ nil))))]
                   (process:send ponger [me :ping])
                   (let [reply (process:recv)]
                     (assert (= reply :pong) "ping-pong: reply is :pong")))))
(println "  1. ping-pong: ok")


# ============================================================================
# 2. Ring of processes
# ============================================================================

(process:start (fn ()
                 (let [me (process:self)]
                   (def @make-forwarder
                     (fn [next]
                       (fn ()
                         (let [msg (process:recv)]
                           (process:send next (+ msg 1))))))

                   (let* [p3 (process:spawn (make-forwarder me))
                          p2 (process:spawn (make-forwarder p3))
                          p1 (process:spawn (make-forwarder p2))]
                     (process:send p1 0)
                     (let [result (process:recv)]
                       (assert (= result 3) "ring: increments message 3 times"))))))
(println "  2. ring: ok")


# ============================================================================
# 3. Fan-in
# ============================================================================

(process:start (fn ()
                 (let [me (process:self)]
                   (def @i 0)
                   (while (< i 5)
                     (let [id i]
                       (process:spawn (fn () (process:send me id))))
                     (assign i (+ i 1)))

                   (def @total 0)
                   (assign i 0)
                   (while (< i 5)
                     (assign total (+ total (process:recv)))
                     (assign i (+ i 1)))

                   (assert (= total 10) "fan-in: 0+1+2+3+4 = 10"))))
(println "  3. fan-in: ok")


# ============================================================================
# 4. Link — crash propagation
# ============================================================================

(process:start (fn ()
                 (process:trap-exit true)
                 (let [me (process:self)]
                   (let [worker-a (process:spawn-link (fn ()
                           (let [b (process:spawn-link (fn ()
                                   (error {:error :boom
                                   :message "worker-b crashed"})))]
                             (process:recv))))]
                     (let [msg (process:recv)]
                       (match msg
                         [:EXIT pid reason] (assert (= pid worker-a)
                         "link-cascade: EXIT from worker-a")
                         _ (assert false "link-cascade: expected EXIT message")))))))
(println "  4. link crash cascade: ok")


# ============================================================================
# 5. trap-exit — convert signals to messages
# ============================================================================

(process:start (fn ()
                 (process:trap-exit true)
                 (let* [me (process:self)
                        child (process:spawn-link (fn ()
                          (error {:error :intentional :message "test crash"})))]
                   (let [msg (process:recv)]
                     (match msg
                       [:EXIT pid reason]
                         (begin
                           (assert (= pid child) "trap-exit: EXIT from child")
                           (match reason
                             [:error _] (assert true
                             "trap-exit: got error reason")
                             _ (assert false "trap-exit: unexpected reason")))
                       _ (assert false "trap-exit: expected EXIT message"))))))
(println "  5. trap-exit: ok")


# ============================================================================
# 6. Normal exit delivers [:EXIT pid [:normal val]]
# ============================================================================

(process:start (fn ()
                 (process:trap-exit true)
                 (let [child (process:spawn-link (fn () 42))]
                   (let [msg (process:recv)]
                     (match msg
                       [:EXIT pid reason]
                         (begin
                           (assert (= pid child) "normal-exit: EXIT from child")
                           (match reason
                             [:normal val] (assert (= val 42)
                             "normal-exit: value is 42")
                             _ (assert false "normal-exit: unexpected reason")))
                       _ (assert false "normal-exit: expected EXIT message"))))))
(println "  6. normal exit: ok")


# ============================================================================
# 7. Unlink prevents notification
# ============================================================================

(process:start (fn ()
                 (process:trap-exit true)
                 (let* [me (process:self)
                        child (process:spawn-link (fn ()
                          (process:recv)
                          (error {:error :boom :message "crash"})))]
                   (process:unlink child)
                   (process:send child :go)
                   (process:send me :still-alive)
                   (let [msg (process:recv)]
                     (assert (= msg :still-alive) "unlink: no EXIT after unlink")))))
(println "  7. unlink: ok")


# ============================================================================
# 8. Fuel-based preemption
# ============================================================================
# A process running an infinite loop gets preempted and other processes
# still make progress.

(process:start (fn ()
                 (let [me (process:self)]
                   (let [busy (process:spawn (fn ()
                           (letrec [loop (fn [n] (loop (+ n 1)))]
                             (loop 0))))]
                     (process:spawn (fn () (process:send me :worker-done)))

                     (let [msg (process:recv)]
                       (assert (= msg :worker-done)
                               "preemption: worker completes despite busy-looper")  # Kill the busy-looper so the scheduler can terminate
                       (process:exit busy :kill))))) :fuel 100)
(println "  8. fuel preemption: ok")


# ============================================================================
# 9. Monitors — [:DOWN ref pid reason]
# ============================================================================

(process:start (fn ()
                 (let* [me (process:self)
                        [child-pid ref] (process:spawn-monitor (fn ()
                          (error {:error :monitored-crash :message "boom"})))]
                   (let [msg (process:recv)]
                     (match msg
                       [:DOWN got-ref got-pid reason]
                         (begin
                           (assert (= got-ref ref) "monitor: correct ref")
                           (assert (= got-pid child-pid) "monitor: correct pid")
                           (match reason
                             [:error _] (assert true "monitor: got error reason")
                             _ (assert false "monitor: unexpected reason")))
                       _ (assert false "monitor: expected DOWN message"))))))
(println "  9. monitors: ok")


# ============================================================================
# 10. Monitor — normal exit
# ============================================================================

(process:start (fn ()
                 (let* [me (process:self)
                        [child-pid ref] (process:spawn-monitor (fn () :done))]
                   (let [msg (process:recv)]
                     (match msg
                       [:DOWN got-ref got-pid reason]
                         (begin
                           (assert (= got-ref ref) "monitor-normal: correct ref")
                           (match reason
                             [:normal val] (assert (= val :done)
                             "monitor-normal: value is :done")
                             _ (assert false "monitor-normal: unexpected reason")))
                       _ (assert false "monitor-normal: expected DOWN message"))))))
(println "  10. monitor normal exit: ok")


# ============================================================================
# 11. Demonitor stops notifications
# ============================================================================

(process:start (fn ()
                 (let* [me (process:self)
                        [child-pid ref] (process:spawn-monitor (fn ()
                          (process:recv)
                          (error {:error :crash :message "crash"})))]
                   (process:demonitor ref)
                   (process:send child-pid :go)
                   (process:send me :still-alive)
                   (let [msg (process:recv)]
                     (assert (= msg :still-alive)
                             "demonitor: no DOWN after demonitor")))))
(println "  11. demonitor: ok")


# ============================================================================
# 12. Named processes — register, whereis, send-named
# ============================================================================

(process:start (fn ()
                 (let [me (process:self)]
                   (process:spawn (fn ()
                                    (process:register :echo-server)
                                    (let [msg (process:recv)]
                                      (match msg
                                        [from payload] (process:send from
                                        [:echo payload])
                                        _ nil))))

                   # Give the echo-server a chance to register by sending ourselves a dummy
                   (process:send me :sync)
                   (process:recv)

                   (let [pid (process:whereis :echo-server)]
                     (assert (not (nil? pid))
                             "named: whereis finds registered process"))

                   (process:send-named :echo-server [me :hello])
                   (let [reply (process:recv)]
                     (match reply
                       [:echo payload] (assert (= payload :hello)
                       "named: echo reply correct")
                       _ (assert false "named: expected echo reply"))))))
(println "  12. named processes: ok")


# ============================================================================
# 13. Process dictionary
# ============================================================================

(process:start (fn ()
                 (let [old (process:put-dict :counter 0)]
                   (assert (nil? old) "dict: put-dict returns nil for new key"))

                 (let [old (process:put-dict :counter 42)]
                   (assert (= old 0) "dict: put-dict returns old value"))

                 (assert (= (process:get-dict :counter) 42)
                         "dict: get-dict returns current value")
                 (assert (nil? (process:get-dict :missing))
                         "dict: get-dict returns nil for missing key")

                 (let [old (process:erase-dict :counter)]
                   (assert (= old 42) "dict: erase-dict returns old value"))

                 (assert (nil? (process:get-dict :counter))
                         "dict: erased key returns nil")))
(println "  13. process dictionary: ok")


# ============================================================================
# 14. Selective receive (recv-match)
# ============================================================================

(process:start (fn ()
                 (let [me (process:self)]
                   (process:send me :a)
                   (process:send me :b)
                   (process:send me :c)

                   # Selectively receive :b first
                   (let [msg (process:recv-match (fn [m] (= m :b)))]
                     (assert (= msg :b) "recv-match: got :b"))

                   # Now receive remaining in order
                   (let [m1 (process:recv)
                         m2 (process:recv)]
                     (assert (= m1 :a) "recv-match: :a preserved in order")
                     (assert (= m2 :c) "recv-match: :c preserved in order")))))
(println "  14. selective receive: ok")


# ============================================================================
# 15. Timers — send-after, cancel-timer
# ============================================================================

(process:start (fn ()
                 (let [me (process:self)]
                   (process:send-after 3 me :timer-fired)

                   # Should not have the message yet (we're at tick ~1)
                   (process:send me :immediate)
                   (let [msg (process:recv)]
                     (assert (= msg :immediate) "timer: immediate message first"))

                   # Now receive — the scheduler will advance ticks and fire the timer
                   (let [msg (process:recv)]
                     (assert (= msg :timer-fired)
                             "timer: delayed message arrived")))))
(println "  15. send-after timer: ok")


# ============================================================================
# 16. Cancel timer
# ============================================================================

(process:start (fn ()
                 (let* [me (process:self)
                        ref (process:send-after 100 me :should-not-arrive)]
                   (let [result (process:cancel-timer ref)]
                     (assert (= result :ok) "cancel-timer: returns :ok"))

                   (process:send me :after-cancel)
                   (let [msg (process:recv)]
                     (assert (= msg :after-cancel)
                             "cancel-timer: cancelled message did not arrive")))))
(println "  16. cancel-timer: ok")


# ============================================================================
# 17. recv-timeout
# ============================================================================

(process:start (fn ()
                 (let [result (process:recv-timeout 1)]
                   (assert (= result :timeout)
                           "recv-timeout: times out when no messages"))))
(println "  17. recv-timeout: ok")


# ============================================================================
# 18. recv-timeout with message available
# ============================================================================

(process:start (fn ()
                 (let [me (process:self)]
                   (process:send me :fast)
                   (let [result (process:recv-timeout 100)]
                     (assert (= result :fast)
                             "recv-timeout: returns message when available")))))
(println "  18. recv-timeout with message: ok")


# ============================================================================
# 19. exit/kill another process
# ============================================================================

(process:start (fn ()
                 (process:trap-exit true)
                 (let* [me (process:self)
                        victim (process:spawn-link (fn ()
                          (process:recv)  # block forever
                          ))]
                   (process:exit victim :test-kill)
                   (let [msg (process:recv)]
                     (match msg
                       [:EXIT pid reason]
                         (begin
                           (assert (= pid victim) "exit-kill: EXIT from victim")
                           (match reason
                             [:killed _] (assert true "exit-kill: killed reason")
                             _ (assert false "exit-kill: unexpected reason")))
                       _ (assert false "exit-kill: expected EXIT message"))))))
(println "  19. exit/kill: ok")


# ============================================================================
# 20. Self-exit
# ============================================================================

(process:start (fn ()
                 (process:trap-exit true)
                 (let [child (process:spawn-link (fn ()
                         (process:exit (process:self) [:normal :voluntary])  # Should not reach here
                         (process:send 999 :unreachable)))]
                   (let [msg (process:recv)]
                     (match msg
                       [:EXIT pid reason] (assert (= pid child)
                       "self-exit: EXIT from child")
                       _ (assert false "self-exit: expected EXIT message"))))))
(println "  20. self-exit: ok")


# ============================================================================
# 21. Monitor doesn't kill watcher
# ============================================================================

(process:start (fn ()
                 (let* [me (process:self)
                        [child ref] (process:spawn-monitor (fn ()
                          (error {:error :crash :message "bang"})))]
                   (let [msg (process:recv)]
                     (match msg
                       [:DOWN _ _ _]  # We're still running — send ourselves proof
                        (process:send me :still-here)
                       _ nil))
                   (let [msg (process:recv)]
                     (assert (= msg :still-here)
                             "monitor-survives: watcher alive after monitored crash")))))
(println "  21. monitor doesn't kill watcher: ok")


# ============================================================================
# 22. External API — inject and process-info
# ============================================================================

(let [sched (process:make-scheduler :backend backend)]
  (def @got-msg nil)
  (process:inject sched 0 :external-msg)  # inject before any process exists — harmless

  (process:run sched
               (fn ()
                 (let [me (process:self)]
                   (process:send me :hello)
                   (assign got-msg (process:recv)))))

  (assert (= got-msg :hello) "external: basic run works")
  (let [info (process:process-info sched 0)]
    (assert (= (get info :status) :dead)
            "external: process-info shows dead after completion")))
(println "  22. external API: ok")


# ============================================================================
# 23. I/O inside processes — println doesn't block other processes
# ============================================================================

(process:start (fn ()
                 (let [me (process:self)]
                   (process:spawn (fn ()
                                    (println "  23. io-process: hello from process A")
                                    (process:send me :a-done)))

                   # Spawn another process that should still make progress
                   (process:spawn (fn () (process:send me :b-done)))

                   (def @got-a false)
                   (def @got-b false)
                   (def @remaining 2)
                   (while (> remaining 0)
                     (match (process:recv)
                       :a-done
                         (begin
                           (assign got-a true)
                           (assign remaining (- remaining 1)))
                       :b-done
                         (begin
                           (assign got-b true)
                           (assign remaining (- remaining 1)))
                       _ nil))
                   (assert got-a "io-process: process A completed")
                   (assert got-b "io-process: process B completed"))))
(println "  23. I/O inside processes: ok")


# ============================================================================
# 24. Structured concurrency inside processes — ev/spawn + ev/join
# ============================================================================

(process:start (fn ()
                 (let* [f1 (ev/spawn (fn () (+ 10 20)))
                        f2 (ev/spawn (fn () (+ 30 40)))
                        r1 (ev/join f1)
                        r2 (ev/join f2)]
                   (assert (= r1 30) "ev/join f1 = 30")
                   (assert (= r2 70) "ev/join f2 = 70"))))
(println "  24. ev/spawn + ev/join inside process: ok")


# ============================================================================
# 25. ev/join-protected inside process — error propagation
# ============================================================================

(process:start (fn ()
                 (let* [f (ev/spawn (fn ()
                                      (error {:error :boom :message "test"})))
                        [ok? val] (ev/join-protected f)]
                   (assert (not ok?) "ev/join-protected: not ok")
                   (assert (= (get val :error) :boom)
                           "ev/join-protected: error is :boom"))))
(println "  25. ev/join-protected inside process: ok")


# ============================================================================
# 26. ev/select inside process
# ============================================================================

(process:start (fn ()
                 (let* [f1 (ev/spawn (fn () :a))
                        f2 (ev/spawn (fn () :b))
                        [done remaining] (ev/select [f1 f2])]
                   (assert (not (nil? done)) "ev/select: got a result"))))
(println "  26. ev/select inside process: ok")


(println "")
(println "all process tests passed.")
