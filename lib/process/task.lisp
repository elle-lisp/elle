(elle/epoch 12)
# audited: 2026-09-23
# Task: one-shot work run as a process, whose result the spawner awaits.
# docs/behaviors.md
#
# A task's value travels in its exit reason, [:normal value], so the
# monitor's :DOWN is the only message a task ever sends its spawner.

(fn [p gs]
  (def {:spawn-monitor spawn-monitor :demonitor demonitor} p)

  (defn task-async [fun]
    "Spawn a monitored process that runs fun. Returns [pid ref], where ref is
     the monitor ref task-await matches."
    (spawn-monitor fun))

  (defn task-await [task &named timeout]
    "Return the value of a task from task-async. Raises {:error :task-error}
     when the task crashed and {:error :task-timeout} once :timeout ticks pass."
    (let [[_ ref] task]
      (match (gs:await-reply ref timeout
                             (fn [m]
                               (and (array? m) (= (length m) 4)
                                    (= (get m 0) :DOWN) (= (get m 1) ref))))
        :timeout
          (begin
            (demonitor ref :flush true)
            (error {:error :task-timeout
                    :message (string "task-await: no result within " timeout
                                     " ticks")}))
        [:DOWN _ _ [:normal value]] value
        [:DOWN _ _ reason]
          (error {:error :task-error :message (string "task crashed: " reason)}))))

  {:task-async task-async :task-await task-await})
