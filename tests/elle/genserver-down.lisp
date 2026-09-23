(elle/epoch 12)
# audited: 2026-09-23
# A server that exits during a call: gen-server-call and gen-server-stop raise :gen-server-down, and the reply leaves no :DOWN.
# docs/behaviors.md

(def process ((import "std/process")))

(defn down-reason [thunk]
  "The :reason of the :gen-server-down that thunk raises. Fails on any other outcome."
  (let [[ok? err] (protect (thunk))]
    (assert (not ok?) (string "expected :gen-server-down, got the value " err))
    (assert (= (get err :error) :gen-server-down)
            (string "expected :gen-server-down, got " err))
    (get err :reason)))

(defn start-crashing-server [name]
  "A server that raises inside every handle-call."
  (process:gen-server-start-link {:init (fn [_] nil)
                                  :handle-call (fn [_req _from _state]
                                    (error {:error :boom
                                    :message "crash in call"}))} nil :name name))

(defn start-stuck-server [name]
  "A server that never finishes a call or a cast."
  (process:gen-server-start-link {:init (fn [_] nil)
                                  :handle-call (fn [_req _from state]
                                    (process:recv-match (fn [m] (= m :never)))
                                    [:reply :late state])
                                  :handle-cast (fn [_req state]
                                    (process:recv-match (fn [m] (= m :never)))
                                    [:noreply state])} nil :name name))

(defn kill-later [pid ticks]
  "Spawn a process that kills pid after ticks."
  (process:spawn (fn []
                   (process:recv-timeout ticks)
                   (process:exit pid :kill))))

# ── the server crashes inside handle-call ────────────────────────────
# The counter-factual: the call waited only for its reply, so the crash
# left it waiting for good and the scheduler raised :deadlock. The caller
# traps exits, and the reply predicate never matched the [:EXIT ...] the
# link delivered.

(process:start (fn []
                 (process:trap-exit true)
                 (let* [server (start-crashing-server :fragile)
                        reason (down-reason (fn []
                          (process:gen-server-call :fragile :ping)))]
                   (assert (= (first reason) :error)
                           "the raise carries the server's exit reason")
                   (assert (= (get (get reason 1) :error) :boom)
                           "and the error the server raised")
                   (assert (= (process:recv-timeout 10) [:EXIT server reason])
                           "the link delivers the same exit as a message"))))

# A caller that is not linked to the server has no link to learn from.
(process:start (fn []
                 (process:trap-exit true)
                 (let [me (process:self)]
                   (start-crashing-server :fragile)
                   (process:spawn (fn []
                                    (process:send me
                                    [:outcome
                                     (protect (process:gen-server-call :fragile
                                     :ping))])))
                   (match (process:recv-match (fn [m]
                       (and (array? m) (= (get m 0) :outcome))))
                     [:outcome [false err]]
                       (assert (= (get err :error) :gen-server-down)
                               "an unlinked caller raises :gen-server-down")
                     other (assert false
                                   (string "expected the call to raise, got "
                                   other))))))

# ── the server is killed mid-call ────────────────────────────────────

(process:start (fn []
                 (process:trap-exit true)
                 (let [server (start-stuck-server :stuck)]
                   (kill-later server 3)
                   (assert (= (down-reason (fn []
                                (process:gen-server-call :stuck :ping)))
                              [:killed :kill])
                           "a call to a server killed mid-call raises with the kill"))))

(process:start (fn []
                 (process:trap-exit true)
                 (let [server (start-stuck-server :stuck)]
                   (process:gen-server-cast :stuck :block)
                   (kill-later server 3)
                   (assert (= (down-reason (fn []
                                (process:gen-server-stop :stuck)))
                              [:killed :kill])
                           "a stop to a server killed before it acknowledges raises"))))

# The deadline does not hide the exit: a call with :timeout raises
# :gen-server-down when the server dies inside the deadline.
(process:start (fn []
                 (process:trap-exit true)
                 (let [server (start-stuck-server :stuck)]
                   (kill-later server 3)
                   (assert (= (down-reason (fn []
                                (process:gen-server-call :stuck :ping
                                :timeout 100))) [:killed :kill])
                           "a server that dies inside the deadline raises :gen-server-down"))))

# ── a call to a server that has already exited ───────────────────────

(process:start (fn []
                 (process:trap-exit true)
                 (let [server (start-stuck-server :gone)]
                   (process:exit server :kill)
                   (process:recv-match (fn [m]
                                         (and (array? m) (= (get m 0) :EXIT))))
                   (let [before (process:now)]
                     (assert (= (down-reason (fn []
                                  (process:gen-server-call server :ping
                                  :timeout 1000))) :noproc)
                             "a call to an exited pid raises with :noproc")
                     (assert (< (- (process:now) before) 10)
                             "at once, not at the deadline"))
                   (assert (= (down-reason (fn []
                                (process:gen-server-stop server))) :noproc)
                           "a stop to an exited pid raises too")
                   (assert (= (down-reason (fn []
                                (process:gen-server-call 9999 :ping))) :noproc)
                           "a call to a pid that never existed raises with :noproc"))))

# A supervisor's client calls are calls: each raises once the supervisor
# has exited.
(process:start (fn []
                 (process:trap-exit true)
                 (let [sup (process:supervisor-start-link [])]
                   (process:exit sup :kill)
                   (process:recv-match (fn [m]
                                         (and (array? m) (= (get m 0) :EXIT))))
                   (assert (= (down-reason (fn []
                                (process:supervisor-which-children sup)))
                              :noproc)
                           "a supervisor call to an exited supervisor raises"))))

# ── the reply leaves no :DOWN behind ─────────────────────────────────
# The call's monitor outlives neither the reply nor the call. The
# counter-factual: a monitor the call never removed delivered :DOWN once
# the server exited, and the caller's next plain recv took it.

(process:start (fn []
                 (process:trap-exit true)
                 (let [server (process:gen-server-start-link {:init (fn [_] 0)
                       :handle-call (fn [_req _from n] [:reply n n])} nil)]
                   (assert (= (process:gen-server-call server :get) 0)
                           "the call returns its reply")
                   (process:exit server :kill)
                   (assert (= (process:recv-timeout 10)
                              [:EXIT server [:killed :kill]])
                           "the link delivers the kill")
                   (assert (= (process:recv-timeout 10) :timeout)
                           "and no :DOWN from the call follows it"))))

(process:start (fn []
                 (process:trap-exit true)
                 (let [server (process:gen-server-start-link {:init (fn [_] nil)
                       :handle-call (fn [_req _from state] [:reply :ok state])}
                       nil)]
                   (assert (= (process:gen-server-stop server) :ok)
                           "the stop returns :ok")
                   (assert (= (process:recv-timeout 10) [:EXIT server :normal])
                           "the link delivers the server's exit")
                   (assert (= (process:recv-timeout 10) :timeout)
                           "and no :DOWN from the stop follows it"))))

(println "genserver-down: ok")
