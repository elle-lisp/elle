(elle/epoch 12)
# audited: 2026-09-08
# The instrument's own self-tests: one live-growth discriminator per gauge, the reclaimed baseline, and the sub-integer estimator check.
#
# docs/impl/region/diagnostics.md
# ── Probes ────────────────────────────────────────────────────────────

# Live-growth discriminator: a genuine unbounded retain. Every op pushes a
# fresh struct into a module-level @array, which keeps it forever, so the gauge
# MUST climb ~1/op. If this does not read :open the gauge is dead.
(def @disc-sink @[])
(defn probe-disc [j]
  (push disc-sink {:k j}))

# The ID gauge's own live-growth discriminator. The object-count discriminator
# above proves nothing about `arena/region-ids`: the two gauges move on different
# events, and an id gauge frozen at whatever the stdlib load left behind would
# read flat for every id probe below and paint each one green. The shape that
# moves it is the same genuine retain — a module-level sink keeps every region it
# is handed, so nothing is freed, the free list drains, and every later mint has
# to take a fresh id. Its own sink, not `disc-sink`: two gauges sharing one sink
# would let either probe's ops satisfy the other's gate.
(def @id-disc-sink @[])
(defn probe-id-disc [j]
  (push id-disc-sink (pair j j)))

# The region gauge's own live-growth discriminator, and the byte gauge's. The
# object-count discriminator proves nothing about either: the gauges move on
# different events, and a region gauge frozen — or filtered down to
# object-holding entries — would read flat for every region pin below and
# paint each one green; a dead byte gauge likewise. The same genuine retain
# moves both: regions never share pages, so each struct a sink keeps forever
# is a live region entry (~1/op) holding one page (~4096 b/op). One sink EACH,
# for the reason the id discriminator states: two gauges sharing one sink
# would let either probe's ops satisfy the other's gate.
(def @region-disc-sink @[])
(defn probe-region-disc [j]
  (push region-disc-sink {:k j}))
(def @bytes-disc-sink @[])
(defn probe-bytes-disc [j]
  (push bytes-disc-sink {:k j}))

# Bounded shape: an immutable struct built and immediately dropped — the
# reclaimed baseline (the leak suite pins this at slope 0). Should read :closed.
(defn probe-bounded [j]
  {:x j :y 2})

# Sub-integer leak: leaks one object every 3 ops = 0.333/op. The OLD integer
# slope floors this to 0 ("reclaimed") — a real unbounded leak made invisible.
# This estimator catches it: measured with a tight tau it reads :open at ≈0.33.
(def @third-sink @[])
(def @third-ctr 0)
(defn probe-third [j]
  (assign third-ctr (%add third-ctr 1))
  (when (%lt 2 third-ctr)  # ctr reached 3
    (assign third-ctr 0)
    (push third-sink {:k 1})))

# ── Run ───────────────────────────────────────────────────────────────
(println "── leak oracle ──")

# 1. Gauge-live gate FIRST. Everything downstream is void if this is not :open.
(def disc (measure "discriminator (live-growth)" probe-disc 200 6 60 0.4 0.5))
(show disc)
(check (assert (= (get disc :verdict) :open)
               (string "GAUGE DEAD: discriminator read " (get disc :verdict)
                       " — every 'closed' verdict this run is void")))

# 1b. The same gate for the ID gauge, which the gate above says nothing about.
(def id-disc
  (measure-core "id discriminator (live-growth)"
                (fn [b] (run-thunk-block probe-id-disc b)) ids-gauge 200 6 60
                0.4 0.5))
(show id-disc)
(check (assert (= (get id-disc :verdict) :open)
               (string "ID GAUGE DEAD: id discriminator read "
                       (get id-disc :verdict)
                       " — every id-gauge 'closed' verdict this run is void")))

# 1c. The same gate for the region gauge, which neither gate above covers.
(def region-disc
  (measure-core "region discriminator (live-growth)"
                (fn [b] (run-thunk-block probe-region-disc b)) region-gauge 200
                6 60 0.4 0.5))
(show region-disc)
(check (assert (= (get region-disc :verdict) :open)
               (string "REGION GAUGE DEAD: region discriminator read "
                       (get region-disc :verdict)
                       " — every region-gauge 'closed' verdict this run is "
                       "void")))

# 1d. The byte gauge's gate. Epsilon and tau are sized in BYTES: a retained
# region costs one page, so a 512 b/op floor against a 1000 b/op tau separates
# a live gauge from a dead one with page-granular noise to spare.
(def bytes-disc
  (measure-core "bytes discriminator (live-growth)"
                (fn [b] (run-thunk-block probe-bytes-disc b)) bytes-gauge 200 6
                60 512.0 1000.0))
(show bytes-disc)
(check (assert (= (get bytes-disc :verdict) :open)
               (string "BYTES GAUGE DEAD: bytes discriminator read "
                       (get bytes-disc :verdict)
                       " — every bytes-gauge 'closed' verdict this run is "
                       "void")))

# 2. Bounded baseline — must reclaim.
(def bnd
  (measure "bounded (immutable struct, dropped)" probe-bounded 200 6 60 0.4 0.5))
(show bnd)
(check (assert (= (get bnd :verdict) :closed)
               (string "bounded shape leaked: " (get bnd :verdict) " rate="
                       (get bnd :rate))))

# 3. Sub-integer leak the integer slope cannot see. Tight tau/epsilon; reads
#    :open at ≈0.33 where `slope` reported 0.
(def sub (measure "sub-integer (1-in-3 retain)" probe-third 300 8 200 0.05 0.1))
(show sub)
(check (assert (= (get sub :verdict) :open)
               (string "sub-integer leak floored to " (get sub :verdict)
                       " rate=" (get sub :rate)
                       " — the estimator must catch what "
                       "integer slope cannot")))
(check (assert (and (< 0.28 (get sub :rate)) (< (get sub :rate) 0.40))
               (string "sub-integer rate " (get sub :rate)
                       " ∉ [0.28,0.40] (expect 0.33)")))
