(elle/epoch 12)
# h2 request loops written so escape analysis can scope them.
#
# `each` desugars to a fiber, which blocks escape analysis; `while` with
# let-bound loop vars keeps each iteration's allocations inside a scope the
# compiler can reclaim. Every loop here is a `while` for that reason.
#
# What each case asserts is the status, the body length that came back, and an
# empty stream table at the end. The `arena/bytes` deltas around them are
# printed for a reader, not asserted.
#
# The counts are named below and kept small on purpose: none of the assertions
# needs volume, and the cost is per-request. The shape this file had before —
# 200 requests of 50k, then 200 of 10k — ran eight seconds here and outran a
# 30 s budget on a CI runner.

(def http2 ((import "std/http2")))

(def seq-requests 60)
(def reconnect-cycles 5)
(def reconnect-requests 10)
(def durability-requests 60)

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

# Each label reads the same constants its case does, so none of them can
# report a shape that did not run.

(println "sequential " seq-requests "x50k...")
(def before (arena/bytes))
(test-sequential-scoped seq-requests body-50k)
(def after (arena/bytes))
(println "  arena delta: " (- after before) " bytes")

(println "reconnect " reconnect-cycles "x" reconnect-requests "...")
(def before2 (arena/bytes))
(test-reconnect-scoped reconnect-cycles reconnect-requests)
(def after2 (arena/bytes))
(println "  arena delta: " (- after2 before2) " bytes")

(println "durability " durability-requests "x10k...")
(def before3 (arena/bytes))
(test-durability-scoped durability-requests body-10k)
(def after3 (arena/bytes))
(println "  arena delta: " (- after3 before3) " bytes")

(println "all scoped h2 stress tests passed")
