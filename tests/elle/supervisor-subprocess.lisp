(elle/epoch 12)
# audited: 2026-09-23
# make-subprocess-child: a supervised OS subprocess, whose exit code decides how its child exits.
# docs/behaviors.md

(def process ((import "std/process")))

(defn exit-reason [spec]
  "Supervise spec, and return the reason its child exits with."
  (def @reason nil)
  (process:start (fn []
                   (let [me (process:self)]
                     (process:supervisor-start-link [spec]
                     :logger (fn [e]
                               (when (= (get e :event) :child-exited)
                                 (process:send me (get e :reason)))))
                     (assign reason (process:recv-timeout 200)))))
  reason)

# The counter-factual: without :opts the child passed @{} to subprocess/exec,
# which takes only an immutable struct, so every start crashed before the
# subprocess ran.
(match (exit-reason (process:make-subprocess-child :ok "true" [] :restart
                    :temporary))
  [:normal _] (assert true "a subprocess that exits 0 ends its child normally")
  other (assert false (string "expected a normal exit, got " other)))

(match (exit-reason (process:make-subprocess-child :fails "false" [] :restart
                    :temporary))
  [:error e]
    (begin
      (assert (= (get e :error) :subprocess-exit)
              "a failing subprocess crashes its child")
      (assert (= (get e :code) 1) "with the exit code"))
  other (assert false (string "expected a :subprocess-exit crash, got " other)))

(match (exit-reason (process:make-subprocess-child :with-opts "true" [] :restart
                    :temporary :opts {}))
  [:normal _] (assert true "explicit :opts reach subprocess/exec")
  other (assert false (string "expected a normal exit, got " other)))

(println "supervisor-subprocess: ok")
