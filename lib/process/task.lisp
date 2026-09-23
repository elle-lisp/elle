(elle/epoch 12)
# audited: 2026-09-23
# Task: one-shot work run as a process, whose result the spawner awaits.
# docs/behaviors.md

(fn [p gs]
  (def {:send send
        :self self
        :recv-match recv-match
        :spawn-monitor spawn-monitor
        :send-after send-after
        :cancel-timer cancel-timer} p)
  (def gen-make-ref gs:gen-make-ref)

  (defn task-async [fun]
    "Spawn a monitored process that runs fun. Returns [pid ref], where ref is
     the monitor ref task-await matches."
    (let* [me (self)
           ref (gen-make-ref)
           [child-pid mon-ref] (spawn-monitor (fn []
             (let [result (fun)]
               (send me [:$task-result ref result]))))]
      [child-pid ref]))

  (defn task-await [task &named timeout]
    "Return the value of a task from task-async. Raises {:error :task-error}
     when the task crashed and {:error :task-timeout} once :timeout ticks pass."
    (let* [ref (get task 1)
           timer-ref (when (not (nil? timeout))
                       (send-after timeout (self) [:$call-timeout ref]))]
      (let [reply (recv-match (fn [m]
                                (and (array? m) (>= (length m) 3)
                                     (or (and (= (get m 0) :$task-result)
                                     (= (get m 1) ref))
                                     (and (= (get m 0) :DOWN)
                                     (= (get m 1) (get task 1))))
                                     (or (nil? timer-ref)
                                     (and (= (get m 0) :$call-timeout)
                                     (= (get m 1) ref)) true))))]
        (when (not (nil? timer-ref)) (cancel-timer timer-ref))
        (match reply
          [:$task-result _ value] value
          [:$call-timeout _ _] (error {:error :task-timeout
                                       :message "task-await timed out"})
          [:DOWN _ _ reason]
            (error {:error :task-error :message (string "task crashed: " reason)})
          _ (error {:error :task-error :message "unexpected task reply"})))))

  {:task-async task-async :task-await task-await})
