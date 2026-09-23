(elle/epoch 12)
# audited: 2026-09-23
# GenServer and Actor: calls, casts, info messages, stops and deferred replies.
# docs/behaviors.md

(def process ((import "std/process")))


# ============================================================================
# GenServer
# ============================================================================

# ── 1. Basic call/reply ───────────────────────────────────────────────

(process:start (fn []
                 (let [pid (process:gen-server-start-link {:init (fn [arg] arg)
                       :handle-call (fn [request _from state]
                                      (match request
                                        :get [:reply state state]
                                        [:set v] [:reply :ok v]
                                        _ [:reply :unknown state]))} 42)]
                   (assert (= 42 (process:gen-server-call pid :get))
                           "call: get initial state")
                   (process:gen-server-call pid [:set 99])
                   (assert (= 99 (process:gen-server-call pid :get))
                           "call: state updated"))))
(println "  1. basic call/reply: ok")


# ── 2. Named server ──────────────────────────────────────────────────

(process:start (fn []
                 (process:gen-server-start-link {:init (fn [_] 0)
                 :handle-call (fn [request _from state]
                                (case request
                                  :inc [:reply (+ state 1) (+ state 1)]
                                  :get [:reply state state]))} nil :name
                 :counter)

                 (process:gen-server-call :counter :inc)
                 (process:gen-server-call :counter :inc)
                 (process:gen-server-call :counter :inc)
                 (assert (= 3 (process:gen-server-call :counter :get))
                         "named: counter is 3")))
(println "  2. named server: ok")


# ── 3. Cast (async) ──────────────────────────────────────────────────

(process:start (fn []
                 (process:gen-server-start-link {:init (fn [_] @[])
                 :handle-call (fn [request _from state]
                                (case request
                                  :get [:reply (freeze state) state]))
                 :handle-cast (fn [request state]
                                (match request
                                  [:push val] (begin
                                    (push state val)
                                    [:noreply state])
                                  _ [:noreply state]))} nil :name :log)

                 (process:gen-server-cast :log [:push :a])
                 (process:gen-server-cast :log [:push :b])
                 (process:gen-server-cast :log [:push :c])  # call to sync — ensures casts have been processed
                 (let [result (process:gen-server-call :log :get)]
                   (assert (= result [:a :b :c]) "cast: three items logged"))))
(println "  3. cast: ok")


# ── 4. Stop with terminate callback ──────────────────────────────────
# The server exits with :shutdown, which kills a starter that does not trap
# exits. The counter-factual: that starter used to die without a trace, so
# every assert after the stop went unrun and the test passed anyway.

(process:start (fn []
                 (process:trap-exit true)
                 (let [me (process:self)
                       server (process:gen-server-start-link {:init (fn [_]
                         :running)
                       :handle-call (fn [request _from state]
                                      [:reply state state])
                       :terminate (fn [reason state]
                                    (process:send me [:terminated reason state]))}
                       nil :name :stoppable)]
                   (assert (= :running (process:gen-server-call :stoppable
                              :status)) "stop: server running")
                   (process:gen-server-stop :stoppable :reason :shutdown)
                   (let [msg (process:recv)]
                     (match msg
                       [:terminated reason state]
                         (begin
                           (assert (= reason :shutdown)
                                   "stop: reason is :shutdown")
                           (assert (= state :running)
                                   "stop: state passed to terminate"))
                       _ (assert false "stop: expected terminated message")))
                   (assert (= (process:recv) [:EXIT server :shutdown])
                           "stop: the server's exit reaches its trapping starter"))))
(println "  4. stop + terminate: ok")


# ── 5. handle-info for non-protocol messages ──────────────────────────

(process:start (fn []
                 (let [me (process:self)]
                   (let [pid (process:gen-server-start-link {:init (fn [_] @[])
                         :handle-call (fn [request _from state]
                                        (case request
                                          :get [:reply (freeze state) state]))
                         :handle-info (fn [msg state]
                                        (push state msg)
                                        [:noreply state])} nil)]
                     (process:send pid :hello)
                     (process:send pid :world)  # Sync via call
                     (let [result (process:gen-server-call pid :get)]
                       (assert (= result [:hello :world])
                               "handle-info: captured messages"))))))
(println "  5. handle-info: ok")


# ── 6. Init with [:ok state] form ────────────────────────────────────

(process:start (fn []
                 (let [pid (process:gen-server-start-link {:init (fn [arg]
                         [:ok (* arg 10)])
                       :handle-call (fn [request _from state]
                                      [:reply state state])} 5)]
                   (assert (= 50 (process:gen-server-call pid :get))
                           "init [:ok state]: state is 50"))))
(println "  6. init [:ok state]: ok")


# ── 7. Deferred reply via gen-server-reply ────────────────────────────
# The server sends the answer to itself from handle-call, so the answer
# always follows the call. The trap: an answer from a separate process
# can reach the server before the call does, and handle-info then runs on
# the nil state and crashes the server.

(process:start (fn []
                 (let [pid (process:gen-server-start-link {:init (fn [_] nil)
                       :handle-call (fn [request from state]  # Stash the caller, reply later from handle-info
                                      (process:send (process:self) :the-answer)
                                      [:noreply from])
                       :handle-info (fn [msg state]  # state is the stashed [pid ref] from the call
                                      (process:gen-server-reply state msg)
                                      [:noreply nil])} nil)]
                   (let [result (process:gen-server-call pid :anything)]
                     (assert (= result :the-answer)
                             "deferred reply: got :the-answer")))))
(println "  7. deferred reply: ok")


# ── 8. Stop from handle-call ─────────────────────────────────────────

(process:start (fn []
                 (process:trap-exit true)
                 (let [pid (process:gen-server-start-link {:init (fn [_] :alive)
                       :handle-call (fn [request _from state]
                                      (case request
                                        :die [:stop :killed :goodbye state]
                                        [:reply state state]))} nil)]
                   (let [reply (process:gen-server-call pid :die)]
                     (assert (= reply :goodbye) "stop-from-call: got goodbye"))
                   (match (process:recv)
                     [:EXIT _ _] (assert true "stop-from-call: got EXIT")
                     _ (assert false "stop-from-call: expected EXIT")))))
(println "  8. stop from handle-call: ok")


# ── 9. Stop from handle-cast ─────────────────────────────────────────

(process:start (fn []
                 (process:trap-exit true)
                 (let [pid (process:gen-server-start-link {:init (fn [_] nil)
                       :handle-cast (fn [request state]
                                      [:stop :cast-shutdown state])} nil)]
                   (process:gen-server-cast pid :bye)
                   (match (process:recv)
                     [:EXIT _ _] (assert true "stop-from-cast: got EXIT")
                     _ (assert false "stop-from-cast: expected EXIT")))))
(println "  9. stop from handle-cast: ok")


# ============================================================================
# Actor
# ============================================================================

# ── 10. Actor get/update ──────────────────────────────────────────────

(process:start (fn []
                 (process:actor-start-link (fn [] 0) :name :counter)
                 (assert (= 0 (process:actor-get :counter (fn [s] s)))
                         "actor: initial 0")
                 (process:actor-update :counter (fn [s] (+ s 1)))
                 (process:actor-update :counter (fn [s] (+ s 1)))
                 (process:actor-update :counter (fn [s] (+ s 1)))
                 (assert (= 3 (process:actor-get :counter (fn [s] s)))
                         "actor: 3 after 3 incs")))
(println "  10. actor get/update: ok")


# ── 11. Actor async cast ─────────────────────────────────────────────

(process:start (fn []
                 (process:actor-start-link (fn [] @[]) :name :items)
                 (process:actor-cast :items (fn [s]
                                       (push s :x)
                                       s))
                 (process:actor-cast :items (fn [s]
                                       (push s :y)
                                       s))  # sync to drain
                 (let [result (process:actor-get :items (fn [s] (freeze s)))]
                   (assert (= result [:x :y]) "actor-cast: items are [:x :y]"))))
(println "  11. actor cast: ok")


# ── 12. Actor derived read ───────────────────────────────────────────

(process:start (fn []
                 (process:actor-start-link (fn [] {:name "elle" :version 1})
                 :name :meta)
                 (assert (= "elle"
                            (process:actor-get :meta (fn [s] (get s :name))))
                         "actor: derived read :name")
                 (assert (= 1
                            (process:actor-get :meta (fn [s] (get s :version))))
                         "actor: derived read :version")))
(println "  12. actor derived read: ok")


(println "genserver: ok")
