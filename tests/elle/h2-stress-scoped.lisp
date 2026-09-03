(elle/epoch 12)
# h2 request loops written so escape analysis can scope them, and the heap
# residue those loops leave behind.
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
# The same request loop runs at two counts over one session, and each drive's
# heap delta is held under `ceiling × requests`. Two counts rather
# than one, because a ceiling on a single delta admits any shape that fits
# under it: a residue that grows faster than the request count passes at the
# small count and fails at the large one, which is the whole reason the large
# one is here. A rate of 0 at both is what "bounded" means.
#
# The gauges are `arena/count` (live objects summed across active regions) and
# `arena/region-count` (active region entries) — the live per-region reads
# `arena-count.lisp` argues for. `arena/bytes` is deliberately NOT read: it
# adds the page pool's cached bytes to the regions' own, so growth in it
# belongs to neither until a second gauge says which, and its page geometry
# makes it swing with the body size while the residue does not.
#
# Both drives share one session, so connecting is outside every window; each
# window is also preceded by an uncounted run, for the reason recorded at
# `residue` below.
#
# ── The pins ─────────────────────────────────────────────────────────
#
# `max-objects-per-request` and `max-regions-per-request` are ceilings on the
# measured rate. They are shrink-only: a change that reclaims more lowers
# them, and nothing may raise them. Both reach 0 when a request leaves nothing
# behind.
#
# ── The gauge-live gate ──────────────────────────────────────────────
#
# A ceiling passes for two reasons: the loop reclaims, or the gauge is dead. A
# dead gauge reads flat and paints every leak green, so a known unbounded
# shape — a module-level sink that keeps every value handed to it — is
# measured first, through the same helper, and must read at least one object
# and one region per run. If that gate fails, every ceiling below is void.
#
# ── The counts ───────────────────────────────────────────────────────
#
# The counts are named below and kept small on purpose: none of the protocol
# assertions needs volume, the cost is per request, and this file shares the
# corpus's per-file budget.

(def http2 ((import "std/http2")))

(def seq-requests 60)
(def reconnect-cycles 5)
(def reconnect-requests 10)
(def durability-requests 60)

# The residue drive's two counts, and the per-request ceilings a drive's heap
# delta must fit under. Shrink-only — see "The pins" above.
(def residue-small 10)
(def residue-large 30)
(def max-objects-per-request 51)
(def max-regions-per-request 45)

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

# ── The residue gauge ────────────────────────────────────────────────
#
# Objects and regions still live after `n` runs of `body`, as a raw delta. The
# caller divides by nothing: an integer division would floor a sub-integer
# rate to 0 and report a real leak as reclaimed, so the delta is compared
# against `ceiling × n` instead and the arithmetic stays exact.
#
# Both gauges are Immediate primitives, so sampling them allocates nothing and
# cannot perturb what they read.
#
# One run happens ahead of every window and is not counted. A window opened
# straight after other work reads 13 objects and 13 regions above the same
# window opened after a run of `body`, whatever `n` is — a one-off the first
# run of the window absorbs, not a per-run cost. Paying it outside the window
# is what makes the rate reproducible: with the lead run, a drive of n
# requests reads exactly n times the same number, every time.

(defn residue [n body]
  (body 0)
  (let [c0 (arena/count)
        r0 (arena/region-count)]
    (def @i 0)
    (while (< i n)
      (body i)
      (assign i (+ i 1)))
    [(- (arena/count) c0) (- (arena/region-count) r0)]))

(defn check-residue [label n body max-objects max-regions]
  (let [[objects regions] (residue n body)]
    # Printed before the asserts, so a run that fails the second count still
    # shows what the first one read.
    (println "  " label " n=" n ": " objects " objects, " regions " regions")
    (assert (<= objects (* max-objects n))
            (string label " n=" n ": " objects " objects over " n
                    " requests exceeds the " max-objects
                    "/request ceiling (budget " (* max-objects n) ")"))
    (assert (<= regions (* max-regions n))
            (string label " n=" n ": " regions " regions over " n
                    " requests exceeds the " max-regions
                    "/request ceiling (budget " (* max-regions n) ")"))
    [objects regions]))

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
                 (let [send-one (fn [i]
                                  (let [resp (http2:send session "POST" "/echo"
                                        :body residue-body)]
                                    (assert (= resp:status 200)
                                    (string "residue: request " i))))]
                   (check-residue "sequential" residue-small send-one
                                  max-objects-per-request
                                  max-regions-per-request)
                   (check-residue "sequential" residue-large send-one
                                  max-objects-per-request
                                  max-regions-per-request))
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

# The gauge-live gate runs before any ceiling it makes meaningful. The sink is
# module-level, so nothing it is handed is ever reclaimed and both gauges must
# climb at least one per run.

(println "gauge-live gate...")
(def @gauge-sink @[])
(defn gauge-growth [i]
  (push gauge-sink {:k i}))

(let [[objects regions] (residue residue-large gauge-growth)]
  (println "  live-growth n=" residue-large ": " objects " objects, " regions
           " regions")
  (assert (<= residue-large objects)
          (string "OBJECT GAUGE DEAD: an unbounded shape read " objects
                  " objects over " residue-large
                  " runs — every residue ceiling this run is void"))
  (assert (<= residue-large regions)
          (string "REGION GAUGE DEAD: an unbounded shape read " regions
                  " regions over " residue-large
                  " runs — every residue ceiling this run is void")))

(println "residue " residue-small "/" residue-large "...")
(test-residue-scoped)

(println "all scoped h2 stress tests passed")
