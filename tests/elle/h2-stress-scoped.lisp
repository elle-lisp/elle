(elle/epoch 12)
# h2 stress test — scoped version
#
# Demonstrates bounded memory under repeated HTTP/2 request loops
# by using while loops (not each) so escape analysis can insert
# scope marks. The key insight: `each` desugars to a fiber,
# which blocks escape analysis. `while` with let-bound loop vars
# keeps allocations within reclaimable scopes.

(def http2 ((import "std/http2")))

# ── Helpers ──────────────────────────────────────────────────────────

(defn listen-ephemeral []
  (let* [listener (tcp/listen "127.0.0.1" 0)
         lpath (port/path listener)
         lport (parse-int (slice lpath (+ 1 (string/find lpath ":"))))]
    [listener lport]))

(defn make-body [size]
  (let [@chunks @[]]
    (def @i 0)
    (while (< i (/ size 20))
      (push chunks (bytes 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19))
      (assign i (+ i 1)))
    (apply concat chunks)))

(defn make-handler []
  (fn [req] {:status 200 :body (or req:body (bytes "ok"))}))

(defn with-server [handler test-fn]
  (let* [[listener lport] (listen-ephemeral)
         sf (ev/spawn (fn []
                        (let [[ok? _] (protect (http2:serve listener handler))]
                          nil)))
         session (http2:connect (concat "http://127.0.0.1:" (string lport)))]
    (defer
      (begin
        (protect (http2:close session))
        (protect (port/close listener))
        (protect (ev/abort sf)))
      (test-fn session))))

# ── Test: sequential requests with scoped response ──────────────────
#
# while loop instead of each. The let binding for resp scopes the
# response struct. concat for assertion messages is replaced by
# string (which formats without intermediates).

(defn test-sequential-scoped [n body]
  (with-server (make-handler)
               (fn [session]
                 (def @i 0)
                 (while (< i n)
                   (let [resp (http2:send session "POST" "/echo" :body body)]
                     (assert (= resp:status 200) (string "seq: request " i))
                     (assert (= (length resp:body) (length body))
                             (string "seq: body size " i)))
                   (assign i (+ i 1)))
                 (assert (= (length (keys session:streams)) 0)
                         "seq: no stream leak")
                 true)))

# ── Test: reconnect cycles with scoped session ──────────────────────

(defn test-reconnect-scoped [cycles reqs-per-cycle]
  (let* [[listener lport] (listen-ephemeral)
         handler (make-handler)
         sf (ev/spawn (fn []
                        (let [[ok? _] (protect (http2:serve listener handler))]
                          nil)))
         url (concat "http://127.0.0.1:" (string lport))]
    (defer
      (begin
        (protect (port/close listener))
        (protect (ev/abort sf)))
      (def @c 0)
      (while (< c cycles)
        (let [session (http2:connect url)]
          (def @j 0)
          (while (< j reqs-per-cycle)
            (let [resp (http2:send session "GET" (string "/fixed?c=" c "&j=" j))]
              (assert (= resp:status 200) (string "reconnect: c=" c " j=" j)))
            (assign j (+ j 1)))
          (http2:close session))
        (assign c (+ c 1)))
      true)))

# ── Test: session durability (many requests, one session) ────────────
#
# The response body is built once, outside the handler: the durability
# property under test is the session surviving many requests, and the
# whole file must fit the test runner's per-tier budget — a per-request
# server-side body build costs more than the request itself.

(defn test-durability-scoped [n body]
  (let [resp-body (make-body (length body))]
    (with-server (fn [req] {:status 200 :body resp-body})
                 (fn [session]
                   (def @i 0)
                   (while (< i n)
                     (let [resp (http2:send session "POST" "/echo" :body body)]
                       (assert (= resp:status 200) (string "durability: req " i)))
                     (assign i (+ i 1)))
                   (assert (= (length (keys session:streams)) 0)
                           "durability: no stream leak")
                   true))))

# ── Run ──────────────────────────────────────────────────────────────

(def body-10k (make-body 10000))
(def body-50k (make-body 50000))

(println "sequential 200x50k...")
(def before (arena/bytes))
(test-sequential-scoped 200 body-50k)
(def after (arena/bytes))
(println "  arena delta: " (- after before) " bytes")

(println "reconnect 10x20...")
(def before2 (arena/bytes))
(test-reconnect-scoped 10 20)
(def after2 (arena/bytes))
(println "  arena delta: " (- after2 before2) " bytes")

(println "durability 200x10k...")
(def before3 (arena/bytes))
(test-durability-scoped 200 body-10k)
(def after3 (arena/bytes))
(println "  arena delta: " (- after3 before3) " bytes")

(println "all scoped h2 stress tests passed")
