(elle/epoch 12)
# audited: 2026-09-23
# A :start-link child spec: a supervisor adopts the process a function spawns, such as a GenServer.
# docs/behaviors.md

(def process ((import "std/process")))

(defn await [pred ticks]
  "The first message matching pred, or :deadline once ticks pass."
  (let [timer (process:send-after ticks (process:self) :deadline)
        msg (process:recv-match (fn [m] (or (= m :deadline) (pred m))))]
    (process:cancel-timer timer)
    msg))

(defn event? [name]
  (fn [m] (and (struct? m) (= (get m :event) name))))

(defn start-kv []
  (process:gen-server-start-link {:init (fn [_] @{})
                                  :handle-call (fn [req _from state]
                                    (match req
                                      [:put k v] (begin
                                        (put state k v)
                                        [:reply :ok state])
                                      [:get k] [:reply (get state k) state]))
                                  :handle-cast (fn [_req _state]
                                    (error {:error :boom :message "crash"}))}
                                 nil :name :kv))

# ── the supervisor watches the server, not the function ──────────────
# The counter-factual: a :start child that called gen-server-start-link was
# a closure that returned at once, so a :permanent spec restarted it without
# end while each server died with the closure that spawned it.

(process:start (fn []
                 (let [me (process:self)]
                   (process:supervisor-start-link [{:id :kv
                   :restart :permanent
                   :start-link start-kv}] :name :sup
                   :logger (fn [e] (process:send me e)))
                   (let [started (await (event? :child-started) 50)]
                     (process:gen-server-call :kv [:put :lang "elle"])
                     (assert (= (process:gen-server-call :kv [:get :lang])
                                "elle") "the supervised server answers calls")
                     (assert (= (get started :pid) (process:whereis :kv))
                             "the supervisor's child is the server itself")
                     (assert (= (await (event? :child-exited) 30) :deadline)
                             "a live server is not restarted")
                     (assert (= (length (process:supervisor-which-children :sup))
                                1) "the supervisor lists the one server")))))

# supervisor-start-link returns once its children have started, so the
# caller can call the server at once. The counter-factual: the supervisor
# started its children after the caller had run on, and the call found no
# process registered as :kv.
(process:start (fn []
                 (process:supervisor-start-link [{:id :kv
                 :restart :permanent
                 :start-link start-kv}] :name :sup)
                 (process:gen-server-call :kv [:put :lang "elle"])
                 (let [first-pid (process:whereis :kv)]
                   (process:gen-server-cast :kv :crash)
                   (process:recv-timeout 20)
                   (assert (not (= (process:whereis :kv) first-pid))
                           "the supervisor restarts a crashed server")
                   (assert (nil? (process:gen-server-call :kv [:get :lang]))
                           "and the restarted server starts from its init"))))

(process:start (fn []
                 (process:supervisor-start-link [{:id :counter
                 :start-link (fn []
                               (process:actor-start-link (fn [] 0) :name
                               :counter))}
                 {:id :events
                  :start-link (fn []
                                (process:event-manager-start-link :name :events))}]
                 :name :sup)
                 (process:actor-update :counter (fn [n] (+ n 1)))
                 (assert (= (process:actor-get :counter (fn [n] n)) 1)
                         "a supervised actor answers")
                 (assert (= (process:event-manager-which-handlers :events) [])
                         "a supervised event manager answers")))

# A :ready child has reported ready by the time supervisor-start-link returns.
(process:start (fn []
                 (let [me (process:self)]
                   (process:supervisor-start-link [{:id :bridge
                   :ready true
                   :start (fn []
                            (process:send me :bridge-up)
                            (process:supervisor-notify-ready)
                            (process:recv))}] :name :sup)
                   (assert (= (process:recv-timeout 0) :bridge-up)
                           "a :ready child is up before supervisor-start-link returns"))))

# ── a :start-link whose process is already gone ──────────────────────
# The function spawns without a link, and the process exits before the
# supervisor links to it. The link then reports :noproc.

(process:start (fn []
                 (let [me (process:self)]
                   (process:supervisor-start-link [{:id :gone
                   :restart :temporary
                   :start-link (fn [] (process:spawn (fn [] :done)))}] :name
                   :sup :logger (fn [e] (process:send me e)))
                   (let [exited (await (event? :child-exited) 50)]
                     (assert (= (get exited :reason) :noproc)
                             "a child gone before the link exits with :noproc"))
                   (assert (empty? (process:supervisor-which-children :sup))
                           "and a temporary one is forgotten"))))

# ── a :start-link that raises crashes the supervisor ─────────────────

(process:start (fn []
                 (process:trap-exit true)
                 (let [sup (process:supervisor-start-link [{:id :broken
                       :start-link (fn []
                                     (error {:error :boom :message "no start"}))}])]
                   (match (await (fn [m] (and (array? m) (= (get m 0) :EXIT)))
                                 50)
                     [:EXIT from [:error _]] (assert (= from sup)
                     "the supervisor crashes with the raise")
                     other (assert false
                                   (string "expected the supervisor's crash, got "
                                   other))))))

# ── malformed specs ──────────────────────────────────────────────────
# The counter-factual: a malformed spec started, and failed later inside
# the supervisor, far from the caller that wrote it.

(defn rejects? [children]
  (let [[ok? err] (protect (process:supervisor-start-link children))]
    (and (not ok?) (= (get err :error) :invalid-child-spec))))

(process:start (fn []
                 (process:trap-exit true)
                 (def body (fn [] (process:recv)))
                 (assert (rejects? [{:start body}]) "a spec needs an :id")
                 (assert (rejects? [{:id :a}])
                         "a spec needs :start or :start-link")
                 (assert (rejects? [{:id :a :start body :start-link body}])
                         "a spec takes one of :start and :start-link")
                 (assert (rejects? [{:id :a :start-link body :ready true}])
                         ":ready goes with :start only")
                 (assert (rejects? [{:id :a :start body} {:id :a :start body}])
                         "ids are unique")
                 (process:supervisor-start-link [{:id :a :start body}] :name
                 :dup-sup)
                 (let [[ok? err] (protect (process:supervisor-start-child :dup-sup {:id :a
                       :start body}))]
                   (assert (not ok?) "a dynamic child may not reuse an id")
                   (assert (= (get err :error) :invalid-child-spec)
                           "as :invalid-child-spec"))
                 (let [[ok? err] (protect (process:supervisor-start-child :dup-sup {:id :b}))]
                   (assert (not ok?)
                           "a dynamic child is checked like a static one")
                   (assert (= (get err :error) :invalid-child-spec)
                           "as :invalid-child-spec"))))

(println "supervisor-start-link: ok")
