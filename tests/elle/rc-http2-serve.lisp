(elle/epoch 11)
# Regression test: http2:serve must not hang with refcounting.
#
# The bug: StoreLocal incref'd ALL values → release_refcounted pinned
# everything → h2 session objects accumulated → queue filled → hang.
# Fix: only StoreLocalRefcounted increfs (mutable bindings only).

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

(println "tests/elle/rc-http2-serve.lisp: passed")
