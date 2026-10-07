(elle/epoch 14)
# audited: 2026-10-06
# A request waiting on its stream when http2:close closes the session raises :connection-closed.
# lib/http2/overview.md
#
# The handler sleeps far longer than the case waits, so the request is
# still parked on its stream's queue when the close arrives. The close
# empties no queue; it closes each one, and a take on a closed, empty
# queue answers nil.
#
# The counter-factual is the request reading a field off that nil: the
# caller gets a type error about `get` on nil, which names no session,
# no stream and no close.

(def http2 ((import "std/http2")))

(def deadline 5)

(defn listen-ephemeral []
  "A listening socket on a kernel-chosen port, with that port."
  (let* [l (tcp/listen "127.0.0.1" 0)
         p (port/path l)]
    [l (parse-int (slice p (+ 1 (string/find p ":"))))]))

(println "a request outlived by its session raises :connection-closed...")

(let* [[listener lport] (listen-ephemeral)
       sf (ev/spawn (fn []
                      (protect (http2:serve listener
                               (fn [req]
                                 (ev/sleep (* 2 deadline))
                                 {:status 200})))))
       sess (http2:connect (concat "http://127.0.0.1:" (string lport)))
       request (ev/spawn (fn [] (http2:send sess "GET" "/slow")))]
  (defer
    (begin
      (protect (ev/abort request))
      (protect (port/close listener))
      (protect (ev/abort sf)))
    (ev/sleep 0.1)
    (http2:close sess)
    (let [r (ev/timeout deadline (fn [] (ev/join-protected request)))]
      (assert (not (nil? r)) "the request must end once its session closes")
      (let [[ok? err] r]
        (assert (not ok?) "the request must fail")
        (assert (= :h2-error (get err :error))
                (string "the failure must be an h2-error, got " (string err)))
        (assert (= :connection-closed (get err :reason))
                (string "the failure must name the closed connection, got "
                        (string err)))))))

(println "h2-send-closed: a closed session ends its requests with :connection-closed")
