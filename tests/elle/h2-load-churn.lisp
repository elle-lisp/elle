(elle/epoch 12)
# One h2 server under connection churn.
#
# Closing a session discards a connection whose stream ids and
# flow-control window are half spent, and the server has to retire the
# reader, the stream table and the socket that went with it. A single
# connect/close pair exercises none of that. Twenty of them, each
# carrying real traffic, do — and what a failure produces is a stall
# rather than an error: a later connect answers with a hang.
#
# So the case below connects, sends 50 requests, closes, and repeats,
# all against one long-lived listener. Every request must be answered
# 200, including the ones on the last connection — those are the check
# that nineteen retirements left the server able to serve a twentieth.
#
# See lib/http2/session.lisp and docs/scheduler.md.

(def http2 ((import "std/http2")))

# A budget no unblocked case here can reach.
(def deadline 60)

(defn listen-ephemeral []
  "A listening socket on a kernel-chosen port, with that port."
  (let* [l (tcp/listen "127.0.0.1" 0)
         p (port/path l)
         port (parse-int (slice p (+ 1 (string/find p ":"))))]
    [l port]))

(defn make-handler []
  "A handler with a fixed route and an echoing default."
  (fn [req]
    (let [path req:path]
      (if (= path "/fixed")
        {:status 200 :body "ok"}
        {:status 200 :body (concat "echo:" path)}))))

(defn timed [label thunk]
  "Run `thunk` under the file's budget and name it."
  (let* [t0 (clock/monotonic)
         r (ev/timeout deadline thunk)
         elapsed (- (clock/monotonic) t0)]
    (assert (not (nil? r))
            (string label ": no result in " (string deadline) " s"))
    (println "  " label ": " (string (round elapsed)) " s")
    r))

# ── Connection churn under load ──────────────────────────────────────

(defn reconnect-cycles []
  "Twenty times: connect, send 50 requests, close."
  (let* [[listener lport] (listen-ephemeral)
         handler (make-handler)
         sf (ev/spawn (fn [] (protect (http2:serve listener handler))))
         url (concat "http://127.0.0.1:" (string lport))]
    (defer
      (begin
        (protect (port/close listener))
        (protect (ev/abort sf)))
      (each cycle in (range 0 20)
        (let [session (http2:connect url)]
          (each i in (range 0 50)
            (let [resp (http2:send session "GET"
                                   (concat "/fixed?c=" (string cycle) "&i="
                                   (string i)))]
              (assert (= resp:status 200)
                      (string "reconnect: cycle " (string cycle) " request "
                              (string i)))))
          (http2:close session)))
      true)))

# ── Run ──────────────────────────────────────────────────────────────

(println "one server under connection churn...")

(timed "20 connect/close cycles of 50 requests" reconnect-cycles)

(println "h2 load churn: every connection was served to its last request")
