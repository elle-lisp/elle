(elle/epoch 14)
# audited: 2026-10-05
# h2 request loops written so escape analysis can scope them, and what those
# loops leave behind per request.
# docs/ratchet.md
#
# `each` desugars to a fiber, which blocks escape analysis; `while` with
# let-bound loop vars keeps each iteration's allocations inside a scope the
# compiler can reclaim. Every loop here is a `while` for that reason.
#
# Three shapes pin protocol behaviour — the response status, the body length
# that came back, and an empty stream table at the end. A fourth reads MEMORY,
# and it is the only one that can say anything about the scoping the loops are
# written for.
#
# ── What the residue drive measures ──────────────────────────────────
#
# The same request loop runs at two counts over one session, and each drive
# reads the objects and regions a request leaves behind, per request, against
# the rows of tests/ledger/h2-stress-scoped.lisp. Two counts rather than one,
# because a reading at a single count admits a residue that appears only past
# it: a residue that grows faster than the request count reads 0 per request at
# the small count and not at the large one, which is the whole reason the
# large one is here. A rate of 0 at both is what "bounded" means.
#
# `arena/bytes` is deliberately NOT read: it adds the page pool's cached bytes
# to the regions' own, so growth in it belongs to neither until a second gauge
# says which, and its page geometry makes it swing with the body size while the
# residue does not. Both drives share one session, so connecting is outside
# every window.
#
# ── The counts ───────────────────────────────────────────────────────
#
# The counts are named below and kept small on purpose: none of the protocol
# assertions needs volume, the cost is per request, and this file shares the
# corpus's per-file budget.

(def http2 ((import "std/http2")))
(def r ((import "std/ratchet")))

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

# ── Test: per-request residue of the sequential loop ─────────────────
#
# The same `while` body the sequential case runs, driven at two counts. The
# body is small because the residue does not scale with it, and a small body
# keeps the drive cheap.

(def residue-body (bytes "residue"))

(defn test-residue-scoped []
  (with-server (make-handler)
               (fn [session]
                 (let [send-one (fn []
                                  (let [resp (http2:send session "POST" "/echo"
                                        :body residue-body)]
                                    (assert (= resp:status 200)
                                    "residue: request")))]
                   (r:delta "sequential, 10 requests" send-one
                            :on [r:objects r:regions] :n 10)
                   (r:delta "sequential, 30 requests" send-one
                            :on [r:objects r:regions] :n 30))
                 true)))

# ── Run ──────────────────────────────────────────────────────────────

(def body-10k (make-body 10000))
(def body-50k (make-body 50000))

# Each label reads the same constants its case does, so none of them can
# report a shape that did not run.

(println "sequential " seq-requests "x50k...")
(test-sequential-scoped seq-requests body-50k)

(println "reconnect " reconnect-cycles "x" reconnect-requests "...")
(test-reconnect-scoped reconnect-cycles reconnect-requests)

(println "durability " durability-requests "x10k...")
(test-durability-scoped durability-requests body-10k)

(println "residue...")
(test-residue-scoped)

(println "all scoped h2 stress tests passed")
