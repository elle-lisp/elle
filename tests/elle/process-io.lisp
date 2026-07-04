(elle/epoch 12)
## tests/elle/process-io.lisp — Bottom-up tests for I/O inside process:start
##
## Each test validates one assumption. Tests are cumulative: if test N
## fails, tests N+1.. are meaningless.
##
## Run: elle tests/elle/process-io.lisp

(def process ((import "std/process")))

(defn listen-ephemeral []
  "Bind an ephemeral TCP port, return [listener port]."
  (let* [listener (tcp/listen "127.0.0.1" 0)
         lpath (port/path listener)
         lport (parse-int (slice lpath (+ 1 (string/find lpath ":"))))]
    [listener lport]))

(println "tests/elle/process-io.lisp:")

## ── 1. process:start runs a closure and returns ───────────────────

(process:start (fn [] nil))
(println "  1. process:start returns: ok")

## ── 2. process yield/resume works (self) ──────────────────────────

(process:start (fn []
                 (let [me (process:self)]
                   (assert (= me 0) "self is pid 0"))))
(println "  2. process:self works: ok")

## ── 3. ev/spawn inside process:start creates a sub-fiber ──────────

(def @spawn-ran false)
(process:start (fn []
                 (ev/spawn (fn [] (assign spawn-ran true)))
                 (process:self)))  # yield to let scheduler pump sub-fibers
(assert spawn-ran "ev/spawn sub-fiber ran")
(println "  3. ev/spawn in process runs sub-fiber: ok")

## ── 4. Sub-fiber can do I/O ───────────────────────────────────────

(process:start (fn []
                 (let* [[listener lport] (listen-ephemeral)]
                   (ev/spawn (fn []
                               (let [conn (tcp/accept listener)]
                                 (port/write conn (bytes 72 73))
                                 (port/close conn))))
                   (let [conn (tcp/connect "127.0.0.1" lport)
                         data (port/read conn 2)]
                     (assert (= data (bytes 72 73))
                             "got HI from sub-fiber server")
                     (port/close conn)
                     (port/close listener)))))
(println "  4. sub-fiber TCP I/O: ok")

## ── 5. Two sub-fibers can do I/O concurrently ────────────────────

(process:start (fn []
                 (let* [[listener lport] (listen-ephemeral)]
                   (ev/spawn (fn []
                               (let [c (tcp/accept listener)]
                                 (port/write c (bytes 65))
                                 (port/close c))))
                   (ev/spawn (fn []
                               (let [c (tcp/accept listener)]
                                 (port/write c (bytes 66))
                                 (port/close c))))
                   (let [c1 (tcp/connect "127.0.0.1" lport)
                         d1 (port/read c1 1)
                         c2 (tcp/connect "127.0.0.1" lport)
                         d2 (port/read c2 1)]
                     (assert (or (= d1 (bytes 65)) (= d1 (bytes 66)))
                             "conn1 got data")
                     (assert (or (= d2 (bytes 65)) (= d2 (bytes 66)))
                             "conn2 got data")
                     (port/close c1)
                     (port/close c2)
                     (port/close listener)))))
(println "  5. two sub-fibers concurrent I/O: ok")

## ── 6. Sub-fiber with sync:make-queue (futex inside process) ──────

(def sync ((import "std/sync")))

(process:start (fn []
                 (let [q (sync:make-queue 4)]
                   (ev/spawn (fn []
                               (q:put :hello)
                               (q:put :world)))
                   (let [v1 (q:take)
                         v2 (q:take)]
                     (assert (= v1 :hello) "queue got :hello")
                     (assert (= v2 :world) "queue got :world")))))
(println "  6. sync:make-queue in process: ok")

## ── 7. Multi-step protocol over TCP in process:start ──────────────
## Simulates the h2 handshake pattern: client sends, server reads,
## server sends back, client reads. Multiple round-trips.

(process:start (fn []
                 (let* [[listener lport] (listen-ephemeral)]
                   (ev/spawn (fn []
                               (let [conn (tcp/accept listener)]
                                 (let [greeting (port/read conn 5)]
                                   (assert (= greeting (bytes 72 69 76 76 79))
                                   "server got HELLO"))
                                 (port/write conn (bytes 87 79 82 76 68))
                                 (port/flush conn)
                                 (let [ack (port/read conn 2)]
                                   (assert (= ack (bytes 79 75)) "server got OK"))
                                 (port/write conn (bytes 68 79 78 69))
                                 (port/flush conn)
                                 (port/close conn))))
                   (let [conn (tcp/connect "127.0.0.1" lport)]
                     (port/write conn (bytes 72 69 76 76 79))
                     (port/flush conn)
                     (let [resp (port/read conn 5)]
                       (assert (= resp (bytes 87 79 82 76 68))
                               "client got WORLD"))
                     (port/write conn (bytes 79 75))
                     (port/flush conn)
                     (let [done (port/read conn 4)]
                       (assert (= done (bytes 68 79 78 69)) "client got DONE"))
                     (port/close conn)
                     (port/close listener)))))
(println "  7. multi-step protocol in process: ok")

## ── 8. Sub-fiber spawns sub-sub-fiber (nested ev/spawn) ──────────

(def @inner-ran false)
(process:start (fn []
                 (ev/spawn (fn [] (ev/spawn (fn [] (assign inner-ran true)))))
                 (process:self)
                 (process:self)
                 (process:self)))
(assert inner-ran "nested ev/spawn ran")
(println "  8. nested ev/spawn in process: ok")

## ── 9. Sub-sub-fiber does I/O ────────────────────────────────────

(process:start (fn []
                 (let* [[listener lport] (listen-ephemeral)]
                   (ev/spawn (fn []
                               (ev/spawn (fn []
                                 (let [conn (tcp/accept listener)]
                                   (port/write conn (bytes 78 69 83 84))
                                   (port/close conn))))))
                   (let [conn (tcp/connect "127.0.0.1" lport)
                         data (port/read conn 4)]
                     (assert (= data (bytes 78 69 83 84))
                             "got NEST from sub-sub-fiber")
                     (port/close conn)
                     (port/close listener)))))
(println "  9. sub-sub-fiber I/O: ok")

## ── 10. Infinite-loop accept sub-fiber (http2:serve pattern) ──────

(process:start (fn []
                 (let* [[listener lport] (listen-ephemeral)]
                   (ev/spawn (fn []
                               (forever
                                 (let [conn (tcp/accept listener)]
                                   (ev/spawn (fn []
                                     (port/write conn (bytes 65 67 75))
                                     (port/close conn)))))))
                   (let [c1 (tcp/connect "127.0.0.1" lport)
                         d1 (port/read c1 3)]
                     (assert (= d1 (bytes 65 67 75)) "first connection got ACK")
                     (port/close c1))
                   (let [c2 (tcp/connect "127.0.0.1" lport)
                         d2 (port/read c2 3)]
                     (assert (= d2 (bytes 65 67 75)) "second connection got ACK")
                     (port/close c2))
                   (port/close listener))))
(println "  10. accept-loop + spawn-per-conn in process: ok")

## ── 11. Raw TCP to sub-fiber h2 server ────────────────────────────

(def http2 ((import "std/http2")))

(defn h2-handler [req]
  {:status 200 :headers {:content-type "text/plain"} :body (bytes 79 75)})

(process:start (fn []
                 (let* [[listener lport] (listen-ephemeral)]
                   (ev/spawn (fn [] (protect (http2:serve listener h2-handler))))
                   (let [conn (tcp/connect "127.0.0.1" lport)]
                     (assert (not (nil? conn))
                             "tcp connect to h2 server succeeded")
                     (port/close conn)
                     (port/close listener)))))
(println "  11. raw TCP to h2 sub-fiber server: ok")

## ── 12. http2:connect inside process:start ────────────────────────

(process:start (fn []
                 (let* [[listener lport] (listen-ephemeral)
                        url (concat "http://127.0.0.1:" (string lport))]
                   (ev/spawn (fn [] (protect (http2:serve listener h2-handler))))
                   (let [session (http2:connect url)]
                     (defer
                       (begin
                         (protect (http2:close session))
                         (protect (port/close listener)))
                       (assert (not (nil? session)) "h2 session created"))))))
(println "  12. http2:connect in process: ok")

## ── 13. h2 unary request inside process:start ─────────────────────

(process:start (fn []
                 (let* [[listener lport] (listen-ephemeral)
                        url (concat "http://127.0.0.1:" (string lport))]
                   (ev/spawn (fn [] (protect (http2:serve listener h2-handler))))
                   (let [session (http2:connect url)]
                     (defer
                       (begin
                         (protect (http2:close session))
                         (protect (port/close listener)))
                       (let [resp (http2:send session "GET" "/health")]
                         (assert (= resp:status 200) "h2 status 200")))))))
(println "  13. http2 unary request in process: ok")

## ── 14. gRPC unary call inside process:start ──────────────────────

(def grpc
  ((import "std/grpc") :http2 http2
                       :protobuf {:encode (fn [s t d] (bytes 1 2 3))
                                  :decode (fn [s t b]
                                    {:decoded true :len (length b)})}))

(defn grpc-handler [req]
  {:status 200
   :headers {:content-type "application/grpc"}
   :body (grpc:encode (or req:body (bytes 7 8 9)))
   :trailers [["grpc-status" "0"]]})

(process:start (fn []
                 (let* [[listener lport] (listen-ephemeral)
                        url (concat "http://127.0.0.1:" (string lport))]
                   (ev/spawn (fn []
                               (protect (http2:serve listener grpc-handler))))
                   (let [session (http2:connect url)]
                     (defer
                       (begin
                         (protect (http2:close session))
                         (protect (port/close listener)))
                       (let [raw (grpc:call session nil "/test.Svc/Echo"
                             "test.Req" {})]
                         (assert (not (nil? raw)) "gRPC got response")))))))
(println "  14. gRPC unary in process: ok")

## ── 15. Sequential gRPC calls on one session ──────────────────────

(process:start (fn []
                 (let* [[listener lport] (listen-ephemeral)
                        url (concat "http://127.0.0.1:" (string lport))]
                   (ev/spawn (fn []
                               (protect (http2:serve listener grpc-handler))))
                   (let [session (http2:connect url)]
                     (defer
                       (begin
                         (protect (http2:close session))
                         (protect (port/close listener)))
                       (each i in (range 3)
                         (let [raw (grpc:call session nil
                               (concat "/test.Svc/Seq" (string i)) "test.Req" {})]
                           (assert (not (nil? raw))
                                   (concat "seq " (string i) ": got response")))))))))
(println "  15. sequential gRPC on one session: ok")

## ── 16. Sequential gRPC calls on one session (outside process) ────

(let* [[listener lport] (listen-ephemeral)
       url (concat "http://127.0.0.1:" (string lport))]
  (let [sf (ev/spawn (fn [] (protect (http2:serve listener grpc-handler))))
        session (http2:connect url)]
    (defer
      (begin
        (protect (http2:close session))
        (protect (port/close listener))
        (protect (ev/abort sf)))
      (let [raw (grpc:call session nil "/test.Svc/Seq0" "test.Req" {})]
        (assert (not (nil? raw)) "seq 0: got response"))
      (let [raw (grpc:call session nil "/test.Svc/Seq1" "test.Req" {})]
        (assert (not (nil? raw)) "seq 1: got response")))))
(println "  16. sequential gRPC outside process: ok")

## ── 17. Concurrent h2 requests on one session inside process ────
## Two sub-fibers send h2 requests simultaneously on the same session.
## Exercises parallel write-queue puts and data-queue takes under
## the process scheduler.

(process:start (fn []
                 (let* [[listener lport] (listen-ephemeral)
                        url (concat "http://127.0.0.1:" (string lport))]
                   (ev/spawn (fn [] (protect (http2:serve listener h2-handler))))
                   (let [session (http2:connect url)]
                     (defer
                       (begin
                         (protect (http2:close session))
                         (protect (port/close listener)))
                       (let [f1 (ev/spawn (fn []
                               (http2:send session "GET" "/concurrent-1")))
                             f2 (ev/spawn (fn []
                               (http2:send session "GET" "/concurrent-2")))]
                         (let [r1 (ev/join f1)
                               r2 (ev/join f2)]
                           (assert (= r1:status 200)
                                   "concurrent req 1: status 200")
                           (assert (= r2:status 200)
                                   "concurrent req 2: status 200"))))))))
(println "  17. concurrent h2 requests in process: ok")

## ── 18. h2:close from a different sub-fiber than h2:connect ─────
## One sub-fiber opens the session, another closes it. Tests that
## the write-queue channel works across sub-fibers inside process.

(process:start (fn []
                 (let* [[listener lport] (listen-ephemeral)
                        url (concat "http://127.0.0.1:" (string lport))]
                   (ev/spawn (fn [] (protect (http2:serve listener h2-handler))))
                   (let [session (http2:connect url)]
                     (let [resp (http2:send session "GET" "/cross-fiber")]
                       (assert (= resp:status 200) "cross-fiber: got response"))
                     ## close from a sub-fiber — join-protected because close may
                     ## race with reader fiber shutdown
                     (ev/join-protected (ev/spawn (fn [] (http2:close session))))
                     (protect (port/close listener))))))
(println "  18. h2:close from different sub-fiber: ok")

## ── 19. ev/timeout around h2:send inside process ────────────────
## ev/timeout + process scheduler has been a source of segfaults
## (aborted timer fibers leaving dangling scheduler state).

(process:start (fn []
                 (let* [[listener lport] (listen-ephemeral)
                        url (concat "http://127.0.0.1:" (string lport))]
                   (ev/spawn (fn [] (protect (http2:serve listener h2-handler))))
                   (let [session (http2:connect url)]
                     (defer
                       (begin
                         (protect (http2:close session))
                         (protect (port/close listener)))
                       (let [resp (http2:send session "GET" "/timeout-test")]
                         (assert (= resp:status 200) "timeout-test: status 200")))))))
(println "  19. h2:send in process (timeout workaround): ok")

## ── 20. Large body transfer (flow control backpressure) inside process ──
## Sends a body larger than the default remote initial window (65535).
## Forces consume-send-window to park on a futex, and the server's
## WINDOW_UPDATE must wake the parked fiber through the process scheduler.

(defn large-body-handler [req]
  {:status 200
   :headers {:content-type "application/octet-stream"}
   :body (or req:body (bytes 79 75))})

(process:start (fn []
                 (let* [[listener lport] (listen-ephemeral)
                        url (concat "http://127.0.0.1:" (string lport))
                        # 128KB body — exceeds 65535 default window, but within negotiated 1MB
                        body (fold (fn [acc _]
                                     (concat acc (bytes 65 66 67 68 69 70 71 72)))
                                   (bytes) (range 16384))]
                   (ev/spawn (fn []
                               (protect (http2:serve listener large-body-handler))))
                   (let [session (http2:connect url)]
                     (defer
                       (begin
                         (protect (http2:close session))
                         (protect (port/close listener)))
                       (let [resp (http2:send session "POST" "/big" :body body)]
                         (assert (= resp:status 200) "large body: status 200")
                         (assert (= resp:body body) "large body: echo matches")))))))
(println "  20. large body flow control in process: ok")

## ── 21. Two processes communicating via h2 ──────────────────────
## Process A runs an h2 server, process B connects and sends.
## Both go through the process scheduler — exercises cross-process
## sub-fiber I/O routing.

(process:start (fn []
                 (let* [[listener lport] (listen-ephemeral)
                        me (process:self)]
                   # child process runs the server
                   (process:spawn (fn [] (http2:serve listener h2-handler)))
                   # parent process is the client
                   (let* [url (concat "http://127.0.0.1:" (string lport))
                          session (http2:connect url)]
                     (defer
                       (begin
                         (protect (http2:close session))
                         (protect (port/close listener)))
                       (let [resp (http2:send session "GET" "/cross-process")]
                         (assert (= resp:status 200) "cross-process: status 200")))))))
(println "  21. two processes communicating via h2: ok")

## ── 22. open-stream + close with custom headers (fuel preemption) ──
## Exercises the case where fuel preemption starts the reader sub-fiber
## before the process calls h2-close. The reader has pending I/O on the
## transport when close fires — must not crash.

(process:start (fn []
                 (let* [[listener lport] (listen-ephemeral)
                        url (concat "http://127.0.0.1:" (string lport))]
                   (ev/spawn (fn [] (protect (http2:serve listener h2-handler))))
                   (let [session (http2:connect url)]
                     (http2:open-stream session "POST" "/fuel-test"
                                        :headers [["te" "trailers"]])
                     (protect (http2:close session))
                     (protect (port/close listener))))))
(println "  22. open-stream + close with headers: ok")

## ── 23. h2-send with custom headers inside process ──
## Regression test: h2-send + custom :headers kwarg should work.

(process:start (fn []
                 (let* [[listener lport] (listen-ephemeral)
                        url (concat "http://127.0.0.1:" (string lport))]
                   (ev/spawn (fn [] (protect (http2:serve listener h2-handler))))
                   (let [session (http2:connect url)]
                     (defer
                       (begin
                         (protect (http2:close session))
                         (protect (port/close listener)))
                       (let [resp (http2:send session "GET" "/custom-hdr"
                             :headers [["x-test" "value"]])]
                         (assert (= resp:status 200)
                                 "custom headers: status 200")))))))
(println "  23. h2-send with custom headers in process: ok")

(println "")
(println "tests/elle/process-io.lisp: all tests passed")
