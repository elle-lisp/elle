(elle/epoch 12)
# audited: 2026-09-23
# When a supervisor restarts a child: early deaths, dynamic children, forgotten specs, and the intensity limit.
# docs/behaviors.md

(def process ((import "std/process")))

(defn await [pred ticks]
  "The first message matching pred, or :deadline once ticks pass."
  (let [timer (process:send-after ticks (process:self) :deadline)
        msg (process:recv-match (fn [m] (or (= m :deadline) (pred m))))]
    (process:cancel-timer timer)
    msg))

(defn is [x]
  (fn [m] (= m x)))

# ── a child that dies before it ever yields ──────────────────────────
# The counter-factual: the supervisor monitored the child in a second
# command, after the child had run and died, so no :DOWN ever came.

(process:start (fn []
                 (let [me (process:self)]
                   (def @lives 0)
                   (process:supervisor-start-link [{:id :eager
                   :restart :permanent
                   :start (fn []
                            (assign lives (+ lives 1))
                            (when (= lives 1)
                              (error {:error :boom :message "first life"}))
                            (process:send me :second-life)
                            (process:recv))}] :name :eager-sup)
                   (assert (= (await (is :second-life) 50) :second-life)
                           "a child that crashes before it yields is restarted"))))

(process:start (fn []
                 (process:supervisor-start-link [{:id :bad
                 :restart :temporary
                 :start (fn [] (error {:error :boom :message "boom"}))}] :name
                 :bad-sup)
                 (process:recv-timeout 20)
                 (assert (empty? (process:supervisor-which-children :bad-sup))
                         "a child that died at once is not listed")))

# ── the intensity limit ──────────────────────────────────────────────
# The counter-factual: the window counted supervisor messages, two per
# crash, so :max-restarts 3 never tripped and the loop never ended.

(process:start (fn []
                 (process:trap-exit true)
                 (let* [me (process:self)
                        sup (process:supervisor-start-link [{:id :flaky
                        :restart :permanent
                        :start (fn []
                                 (process:recv-timeout 1)
                                 (error {:error :boom :message "boom"}))}]
                        :max-restarts 3
                        :logger (fn [e] (process:send me (get e :event))))]
                   (def @restarts 0)
                   (def @outcome nil)
                   (while (nil? outcome)
                     (match (process:recv-timeout 400)
                       :child-restarting
                         (begin
                           (assign restarts (+ restarts 1))
                           (when (> restarts 20) (assign outcome :storm)))
                       [:EXIT from reason] (when (= from sup)
                         (assign outcome reason))
                       :timeout (assign outcome :timeout)
                       _ nil))
                   (assert (= outcome :shutdown)
                           "a supervisor past its limit exits with :shutdown")
                   (assert (= restarts 3) "after exactly :max-restarts restarts"))))

(process:start (fn []
                 (process:trap-exit true)
                 (let [me (process:self)]
                   (process:supervisor-start-link [{:id :steady
                   :restart :permanent
                   :start (fn []
                            (process:send me [:steady (process:self)])
                            (process:recv))}
                   {:id :flaky
                    :restart :permanent
                    :start (fn []
                             (process:recv-timeout 1)
                             (error {:error :boom :message "boom"}))}]
                   :max-restarts 2)
                   (let* [steady (get (process:recv) 1)
                          ref (process:monitor steady)]
                     (assert (= (await (fn [m]
                                         (and (array? m) (= (get m 0) :DOWN)))
                                       400)
                                [:DOWN ref steady [:killed :shutdown]])
                             "a supervisor past its limit shuts its other children down")))))

# The window is in ticks. A child that crashes once every 30 ticks never has
# two restarts inside a 20-tick window, however many messages pass. The
# counter-factual: counting messages tripped the limit at the second restart.
(process:start (fn []
                 (process:trap-exit true)
                 (let* [me (process:self)
                        sup (process:supervisor-start-link [{:id :slow
                        :restart :permanent
                        :start (fn []
                                 (process:send me :slow-started)
                                 (process:recv-timeout 30)
                                 (error {:error :boom :message "boom"}))}]
                        :max-restarts 1 :max-ticks 20
                        :logger (fn [e]
                                  (when (= (get e :event) :max-restarts-reached)
                                    (process:send me :tripped))))]
                   (def @starts 0)
                   (while (< starts 4)
                     (match (await (fn [m]
                                     (or (= m :slow-started) (= m :tripped)))
                                   400)
                       :slow-started (assign starts (+ starts 1))
                       other (assert false
                                     (string "expected four starts, got " other))))
                   (process:exit sup :shutdown)
                   (assert (= (await (fn [m]
                                       (and (array? m) (= (get m 0) :EXIT))) 50)
                              [:EXIT sup :shutdown])
                           "the supervisor ran until it was told to stop"))))

# ── dynamic children ─────────────────────────────────────────────────
# The counter-factual: :one-for-all and :rest-for-one looked a child's spec
# up in the static list, so a dynamic child logged :child-restarting and
# never started again.

(each strategy in [:one-for-one :one-for-all :rest-for-one]
  (process:start (fn []
                   (let [me (process:self)]
                     (def @dyn-lives 0)
                     (process:supervisor-start-link [{:id :steady
                     :restart :permanent
                     :start (fn []
                              (process:send me :steady-started)
                              (process:recv))}] :name :dyn-sup
                     :strategy strategy)
                     (await (is :steady-started) 50)
                     (process:supervisor-start-child :dyn-sup {:id :dyn
                     :restart :permanent
                     :start (fn []
                              (assign dyn-lives (+ dyn-lives 1))
                              (process:send me [:dyn-started dyn-lives])
                              (when (= dyn-lives 1)
                                (process:recv-timeout 2)
                                (error {:error :boom :message "boom"}))
                              (process:recv))})
                     (assert (= (await (is [:dyn-started 1]) 50)
                                [:dyn-started 1]) "the dynamic child starts")
                     (assert (= (await (is [:dyn-started 2]) 50)
                                [:dyn-started 2])
                             (string strategy " restarts a dynamic child"))
                     (when (= strategy :one-for-all)
                       (assert (= (await (is :steady-started) 50)
                                  :steady-started)
                               ":one-for-all restarts the static sibling of a dynamic child"))))))

# ── a spec the supervisor has forgotten ──────────────────────────────
# The counter-factual: :one-for-all restarted every id still in the static
# list, which brought back a stopped child and a finished temporary one.

(defn crash-once [me tag]
  "A child body that reports each start and crashes on :crash, once."
  (def @lives 0)
  (fn []
    (assign lives (+ lives 1))
    (process:send me [tag lives])
    (when (= lives 1)
      (process:recv-match (is :crash))
      (error {:error :boom :message "crash"}))
    (process:recv)))

(defn pid-of [sup id]
  (get (find (fn [c] (= (get c :id) id)) (process:supervisor-which-children sup))
       :pid))

(process:start (fn []
                 (let [me (process:self)]
                   (process:supervisor-start-link [{:id :stopped
                   :restart :permanent
                   :start (fn []
                            (process:send me :stopped-started)
                            (process:recv))}
                   {:id :once
                    :restart :temporary
                    :start (fn []
                             (process:send me :once-started)
                             :done)}
                   {:id :crasher
                    :restart :permanent
                    :start (crash-once me :crasher)}] :name :forget-sup
                   :strategy :one-for-all)
                   (await (is :stopped-started) 50)
                   (await (is :once-started) 50)
                   (await (is [:crasher 1]) 50)
                   (process:supervisor-stop-child :forget-sup :stopped)
                   (process:recv-timeout 5)
                   (process:send (pid-of :forget-sup :crasher) :crash)
                   (assert (= (await (is [:crasher 2]) 50) [:crasher 2])
                           "the crasher restarts")
                   (assert (= (await (fn [m]
                                       (or (= m :stopped-started)
                                       (= m :once-started))) 20) :deadline)
                           "neither a stopped child nor a finished temporary one comes back")
                   (assert (= (map (fn [c] (get c :id))
                                   (process:supervisor-which-children :forget-sup))
                              [:crasher])
                           "the supervisor lists only the child it still has"))))

# ── readiness ────────────────────────────────────────────────────────
# The counter-factual: a child that died before it reported ready was
# dropped, and its restart policy never ran.

(process:start (fn []
                 (let [me (process:self)]
                   (def @bridge-lives 0)
                   (process:supervisor-start-link [{:id :bridge
                   :restart :permanent
                   :ready true
                   :start (fn []
                            (assign bridge-lives (+ bridge-lives 1))
                            (process:send me [:bridge bridge-lives])
                            (when (= bridge-lives 1)
                              (error {:error :boom :message "not ready"}))
                            (process:supervisor-notify-ready)
                            (process:recv))}
                   {:id :client
                    :restart :permanent
                    :start (fn []
                             (process:send me :client-started)
                             (process:recv))}] :name :ready-sup)
                   (assert (= (await (is :client-started) 50) :client-started)
                           "the next child starts after a child dies before ready")
                   (assert (= (await (is [:bridge 2]) 50) [:bridge 2])
                           "and the child that died is restarted"))))

(println "supervisor-restart: ok")
