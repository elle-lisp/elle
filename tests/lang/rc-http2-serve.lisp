(elle/epoch 12)
# audited: 2026-09-29
# http2:serve answers a stream of requests without holding every session object alive.
# lib/http2.md
#
# Only `StoreLocalRefcounted` increfs, for mutable bindings. The counter-factual
# increfs every `StoreLocal`, which pins every value: the h2 session objects
# accumulate, the queue fills, and the server hangs.

(def http2 ((import "std/http2")))

(let* [listener (tcp/listen "127.0.0.1" 0)
       lpath (port/path listener)
       lport (parse-int (slice lpath (+ 1 (string/find lpath ":"))))]
  (def sf
    (ev/spawn (fn []
                (protect (http2:serve listener
                                      (fn [req] {:status 200 :body "ok"}))))))
  (ev/sleep 0.1)
  (def session (http2:connect (concat "http://127.0.0.1:" (string lport))))
  (def resp (http2:send session "GET" "/test"))
  (assert (= resp:status 200) "http2:serve responded")
  (println "status: " resp:status)
  (http2:close session)
  (port/close listener)
  (ev/abort sf))

(println "tests/lang/rc-http2-serve.lisp: passed")
