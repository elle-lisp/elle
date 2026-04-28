(elle/epoch 9)
## tests/http2/scheduler.lisp — h2 client+server in the same scheduler
## See tests/http2/server.lisp for comprehensive server coverage.

(def http2 ((import "std/http2")))

(defn listen-ephemeral []
  (let* [listener (tcp/listen "127.0.0.1" 0)
         lpath (port/path listener)
         lport (parse-int (slice lpath (+ 1 (string/find lpath ":"))))]
    [listener lport]))

(defn with-server [handler test-fn]
  (let* [[listener lport] (listen-ephemeral)
         sf (ev/spawn
           (fn []
             (let [[ok? _] (protect
               (http2:serve listener handler))]
               nil)))
         session (http2:connect (concat "http://127.0.0.1:" (string lport)))]
    (defer (begin (protect (http2:close session))
                  (protect (port/close listener))
                  (protect (ev/abort sf)))
      (test-fn session))))

(defn test-single-request []
  (with-server
    (fn [req] {:status 200 :body (concat "echo:" req:path)})
    (fn [session]
      (let [resp (http2:send session "GET" "/hello")]
        (assert (= resp:status 200)
                (concat "status should be 200, got " (string resp:status)))
        (assert (= (string resp:body) "echo:/hello")
                (concat "body should be echo:/hello, got "
                        (string resp:body))))))
  (println "  PASS: single request"))

(defn test-sequential-requests []
  (with-server
    (fn [req] {:status 200 :body (concat "seq:" req:path)})
    (fn [session]
      (each i in (range 0 10)
        (let [resp (http2:send session "GET" (concat "/req-" (string i)))]
          (assert (= resp:status 200)
                  (concat "seq req " (string i) " status"))
          (assert (= (string resp:body) (concat "seq:/req-" (string i)))
                  (concat "seq req " (string i) " body"))))))
  (println "  PASS: 10 sequential requests"))

(defn test-request-with-body []
  (with-server
    (fn [req]
      {:status 200
       :body (if (nil? req:body) "nobody" (string req:body))})
    (fn [session]
      (let [resp (http2:send session "POST" "/data" :body "hello world")]
        (assert (= resp:status 200)
                (concat "post: status " (string resp:status)))
        (assert (= (string resp:body) "hello world")
                (concat "post: body " (string resp:body))))))
  (println "  PASS: request with body"))

(println "h2 scheduler tests:")
(test-single-request)
(test-sequential-requests)
(test-request-with-body)
(println "all h2 scheduler tests passed")
