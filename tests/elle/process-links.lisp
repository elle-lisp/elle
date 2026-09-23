(elle/epoch 12)
# audited: 2026-09-23
# Exit signals: what links, monitors and exit deliver, and how the end of PID 0 reaches process:start.
# docs/processes.md

(def process ((import "std/process")))

(defn await [pred ticks]
  "The first message matching pred, or :deadline once ticks pass."
  (let [timer (process:send-after ticks (process:self) :deadline)
        msg (process:recv-match (fn [m] (or (= m :deadline) (pred m))))]
    (process:cancel-timer timer)
    msg))

(defn down? [m]
  (and (array? m) (= (get m 0) :DOWN)))

# ── links: the exit signal a trapping process receives ───────────────

# A crash cascades along a chain of links to the trapping end.
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

# ── monitors: the :DOWN a watcher receives ───────────────────────────

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

# ── exit: a signal one process sends another, or itself ──────────────

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

# ── a linked crash reaches process:start ─────────────────────────────
# The counter-factual: a link marked a non-trapping PID 0 dead with no exit
# reason, so process:start returned normally after the crash.

(let [[ok? err] (protect (process:start (fn []
                                          (process:spawn-link (fn []
                                            (error {:error :boom
                                            :message "crash"})))
                                          (process:recv-timeout 20)
                                          :survived)))]
  (assert (not ok?) "a linked crash that kills PID 0 raises from process:start")
  (assert (= (get err :error) :process-error) "the raise is a :process-error")
  (assert (string? (get err :reason)) "carrying the exit reason as a string"))

# ── a kill from another process reaches process:start ────────────────

(let [[ok? err] (protect (process:start (fn []
                                          (process:spawn (fn []
                                            (process:exit 0 :die)))
                                          (process:recv))))]
  (assert (not ok?) "PID 0 killed by another process raises from process:start")
  (assert (= (get err :error) :process-error) "as a :process-error"))

# A reason PID 0 chose itself is not a crash.
(process:start (fn [] (process:exit (process:self) :shutdown)))

# ── a process a link kills goes through the whole exit ───────────────
# The counter-factual: the linked process was marked dead in place, so its
# monitors never heard of it and its name stayed registered.

(process:start (fn []
                 (let* [middle (process:spawn (fn []
                          (process:register :middle)
                          (process:spawn-link (fn []
                            (process:register :crasher)
                            (process:recv)
                            (error {:error :boom :message "crash"})))
                          (process:recv)))
                        ref (process:monitor middle)]
                   (process:recv-timeout 5)
                   (process:send-named :crasher :go)
                   (match (await down? 50)
                     [:DOWN got-ref got-pid [:linked _ [:error _]]]
                       (begin
                         (assert (= got-ref ref)
                                 "the monitor of a link-killed process fires")
                         (assert (= got-pid middle) "for that process")
                         (assert (nil? (process:whereis :middle))
                                 "a link-killed process releases its name"))
                     other (assert false
                                   (string "expected [:DOWN ref middle [:linked ...]], got "
                                   other))))))

# The kill cascades, and each step names the link it came through.
(process:start (fn []
                 (let* [top (process:spawn (fn []
                          (process:spawn-link (fn []
                            (process:spawn-link (fn []
                              (process:recv-timeout 5)
                              (error {:error :boom :message "crash"})))
                            (process:recv)))
                          (process:recv)))
                        ref (process:monitor top)]
                   (match (await down? 50)
                     [:DOWN _ _ [:linked _ [:linked _ [:error _]]]] (assert true
                     "the cascade reaches the far end of the chain")
                     other (assert false
                                   (string "expected a two-link cascade, got "
                                   other))))))

# ── a normal exit reaches only trapping links ────────────────────────
# The counter-factual: a normal exit killed a non-trapping linked peer.

(process:start (fn []
                 (let* [me (process:self)
                        peer (process:spawn (fn []
                          (process:recv)
                          (process:send me :peer-alive)))]
                   (process:spawn (fn []
                                    (process:link peer)
                                    :returned))
                   (process:spawn (fn []
                                    (process:link peer)
                                    (process:exit (process:self) :normal)))
                   (process:recv-timeout 5)
                   (process:send peer :go)
                   (assert (= (await (fn [m] (= m :peer-alive)) 20) :peer-alive)
                           "normal exits leave a non-trapping linked peer running"))))

(process:start (fn []
                 (process:trap-exit true)
                 (let [child (process:spawn-link (fn [] :done))]
                   (assert (= (process:recv) [:EXIT child [:normal :done]])
                           "a trapping link receives the normal exit"))))

# ── monitor and link to a process that has already exited ────────────
# The counter-factual: a monitor on a dead process never fired, and a
# monitor on a pid that never existed crashed the scheduler.

(process:start (fn []
                 (let [[pid _] (process:spawn-monitor (fn [] :done))]
                   (process:recv)
                   (let [ref (process:monitor pid)]
                     (assert (= (await down? 20) [:DOWN ref pid :noproc])
                             "a monitor on an exited process delivers :noproc at once"))
                   (let [ref (process:monitor 999)]
                     (assert (= (await down? 20) [:DOWN ref 999 :noproc])
                             "a monitor on a pid that never existed delivers :noproc")))))

(process:start (fn []
                 (let [[pid _] (process:spawn-monitor (fn [] :done))]
                   (process:recv)
                   (let [[ok? err] (protect (process:link pid))]
                     (assert (not ok?)
                             "link to an exited process raises in a caller that does not trap")
                     (assert (= (get err :error) :noproc) "the raise is :noproc"))
                   (process:trap-exit true)
                   (process:link pid)
                   (assert (= (await (fn [m] true) 20) [:EXIT pid :noproc])
                           "a trapping caller receives [:EXIT pid :noproc]"))))

# A link whose exit already arrived delivers nothing more.
(process:start (fn []
                 (process:trap-exit true)
                 (let [child (process:spawn-link (fn [] :done))]
                   (process:recv)
                   (process:link child)
                   (assert (= (process:recv-timeout 5) :timeout)
                           "relinking an exited partner sends no second exit"))))

# ── demonitor :flush ─────────────────────────────────────────────────
# The monitored process has exited and its :DOWN waits in the mailbox; the
# wait for :tick leaves it there.

(defn exited-monitor []
  "Monitor a process that exits, and return its ref once its :DOWN is queued."
  (let [[_ ref] (process:spawn-monitor (fn [] :done))]
    (process:send-after 5 (process:self) :tick)
    (process:recv-match (fn [m] (= m :tick)))
    ref))

(process:start (fn []
                 (let [ref (exited-monitor)]
                   (process:demonitor ref :flush true)
                   (assert (= (process:recv-timeout 5) :timeout)
                           "demonitor :flush removes a :DOWN already delivered"))))

(process:start (fn []
                 (let [ref (exited-monitor)]
                   (process:demonitor ref)
                   (assert (down? (process:recv-timeout 5))
                           "demonitor without :flush leaves it"))))

# ── the clock ────────────────────────────────────────────────────────

(process:start (fn []
                 (let [t0 (process:now)]
                   (assert (integer? t0) "now is an integer tick")
                   (process:recv-timeout 10)
                   (assert (>= (- (process:now) t0) 10)
                           "recv-timeout 10 moves the clock at least 10 ticks"))))

# ── the end of the program ───────────────────────────────────────────
# The counter-factual: a process left waiting after PID 0 returned made
# process:start raise :deadlock.

(let [sched (process:make-scheduler)
      idle @[]]
  (process:run sched
               (fn []
                 (push idle (process:spawn (fn [] (process:recv))))
                 (push idle
                       (process:spawn (fn []
                                        (process:trap-exit true)
                                        (process:recv-match (fn [m] (= m :never))))))
                 :done))
  (each pid in idle
    (assert (= (get (process:process-info sched pid) :status) :dead)
            "a process idle for good is shut down once PID 0 has ended")))

(let [[ok? err] (protect (process:start (fn [] (process:recv))))]
  (assert (not ok?) "a PID 0 waiting on nothing still deadlocks")
  (assert (= (get err :error) :deadlock) "with :deadlock"))

# ── an unknown wait op ───────────────────────────────────────────────
# The counter-factual: the scheduler raised it, ending every process at once.

(process:start (fn []
                 (let [[ok? err] (protect (emit :wait {:op :no-such-op}))]
                   (assert (not ok?)
                           "an unknown wait op raises in the process that emitted it")
                   (assert (= (get err :error) :protocol-error)
                           "as :protocol-error"))))

(println "process-links: ok")
