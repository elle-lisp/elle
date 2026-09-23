(elle/epoch 12)
# audited: 2026-09-23
# The scheduler's command table: what each yielded process primitive does.
# lib/process/overview.md
# docs/processes.md

(fn [core]
  (def proc-get core:proc-get)
  (def alive? core:alive?)
  (def resume core:resume)
  (def names core:names)

  (defn wait-for-message [pid pred]
    "Block pid until a message matching pred arrives; nil pred takes any."
    (let* [p (proc-get pid)
           found (if (nil? pred)
                   (let [mbox (get p :mbox)]
                     (when (> (length mbox) 0)
                       (let [msg (get mbox 0)]
                         (remove mbox 0)
                         msg)))
                   (let [m (core:scan-mbox pid pred)]
                     (when (not (nil? m)) (core:restore-save-queue pid))
                     m))]
      (if (nil? found)
        (begin
          (put p :recv-pred pred)
          (push core:waiting pid)
          false)
        (begin
          (resume pid found)
          true))))

  (defn handle-cmd [pid cmd]
    (case (get cmd 0)
      :send
        (begin
          (core:deliver (get cmd 1) (get cmd 2))
          (resume pid :ok))
      :recv (wait-for-message pid nil)
      :recv-match (wait-for-message pid (get cmd 1))
      :recv-timeout
        (when (not (wait-for-message pid nil))
          (let [ref (core:add-timer (get cmd 1) pid :timeout)]
            (put (proc-get pid) :timer-ref ref)))
      :self (resume pid pid)
      :spawn
        (resume pid (core:spawn (get cmd 1)))
      :spawn-link
        (let [new-pid (core:spawn (get cmd 1))]
          (core:add-link pid new-pid)
          (resume pid new-pid))
      :spawn-monitor
        (let* [new-pid (core:spawn (get cmd 1))
               ref (core:add-monitor pid new-pid)]
          (resume pid [new-pid ref]))
      :link
        (resume pid (core:link pid (get cmd 1)))
      :unlink
        (begin
          (core:remove-link pid (get cmd 1))
          (resume pid :ok))
      :monitor
        (resume pid (core:add-monitor pid (get cmd 1)))
      :demonitor
        (let [ref (get cmd 1)]
          (core:remove-monitor ref pid)
          (when (get cmd 2) (core:flush-down pid ref))
          (resume pid :ok))
      :make-ref (resume pid (core:fresh-ref))
      :now (resume pid (core:now))
      :trap-exit
        (begin
          (put (proc-get pid) :trapping (get cmd 1))
          (resume pid :ok))
      :exit
        (let [target (get cmd 1)
              reason (get cmd 2)]
          (if (= target pid)
            (core:process-exit pid reason)
            (when (alive? target)
              (if (and (get (proc-get target) :trapping) (not (= reason :kill)))
                (core:deliver target [:EXIT pid reason])
                (core:process-exit target [:killed reason]))))
          (resume pid :ok))
      :register
        (let [name (get cmd 1)]
          (put names name pid)
          (put (proc-get pid) :name name)
          (resume pid :ok))
      :unregister
        (begin
          (del names (get cmd 1))
          (put (proc-get pid) :name nil)
          (resume pid :ok))
      :whereis
        (resume pid (get names (get cmd 1) nil))
      :send-named
        (let [target (get names (get cmd 1) nil)]
          (when target (core:deliver target (get cmd 2)))
          (resume pid :ok))
      :send-after
        (resume pid (core:add-timer (get cmd 1) (get cmd 2) (get cmd 3)))
      :cancel-timer
        (resume pid (if (core:cancel-timer (get cmd 1)) :ok :not-found))
      :put-dict
        (let* [dict (get (proc-get pid) :dict)
               key (get cmd 1)
               old (get dict key nil)]
          (put dict key (get cmd 2))
          (resume pid old))
      :get-dict
        (resume pid (get (get (proc-get pid) :dict) (get cmd 1) nil))
      :erase-dict
        (let* [dict (get (proc-get pid) :dict)
               key (get cmd 1)
               old (get dict key nil)]
          (del dict key)
          (resume pid old))
      (error {:error :protocol-error
              :message (string "unknown scheduler command: " (get cmd 0))})))

  handle-cmd)
