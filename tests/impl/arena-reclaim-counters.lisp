(elle/epoch 13)
# audited: 2026-09-29
# The reclamation counters a program reads: each moves at its event, reading them allocates nothing, and the forest counters close.
# docs/impl/region/diagnostics.md
#
# The counterfactual: a counter stuck at zero reclaims exactly as well as a
# live one, so only a reading that must move can tell them apart.

(def counters
  [arena/region-frees arena/page-frees arena/object-frees arena/adopts
   arena/adopts-into-empty arena/owned-frees arena/owned-free-pages
   arena/owned-free-objects arena/owned-one-page-frees arena/rescues
   arena/rescue-survivors arena/extracts arena/reparents arena/owned])

(defn forest-closes? []
  (let [ended (+ (arena/owned-frees) (arena/rescues) (arena/extracts))]
    (= (arena/adopts) (+ ended (arena/owned)))))

(assert (forest-closes?)
        "adopts = owned-frees + rescues + extracts + owned after the stdlib load")

# ── every counter is an integer, and reading one allocates nothing ────
(each c in counters
  (assert (int? (c)) "every counter answers an integer"))

(defn read-all []
  (each c in counters
    (c)))

(def objects-before (arena/count))
(read-all)
(assert (= (arena/count) objects-before) "reading every counter moves no object")

# ── a push into a local container: one adoption, freed with its owner ──
(defn keep-one []
  (let [c (@array)]
    (push c (array 1 2))
    nil))

(def adopts (arena/adopts))
(def owned-frees (arena/owned-frees))
(def owned (arena/owned))
(keep-one)
(assert (= (arena/adopts) (+ adopts 1))
        "the push adopts the value into the container")
(assert (= (arena/owned-frees) (+ owned-frees 1))
        "the container's drop frees the value")
(assert (= (arena/owned) owned) "nothing is left owned")
(assert (forest-closes?) "the forest counters close after the drop")

# ── a call result freed by its count: region, object and page frees ────
(defn churn [n]
  (var i 0)
  (while (< i n)
    (string "c" i)
    (assign i (+ i 1))))

(def region-frees (arena/region-frees))
(def object-frees (arena/object-frees))
(def page-frees (arena/page-frees))
(churn 100)
(assert (>= (- (arena/region-frees) region-frees) 100)
        "each discarded string's region is counted as it frees")
(assert (>= (- (arena/object-frees) object-frees) 100) "and each one's object")
(assert (>= (- (arena/page-frees) page-frees) 100) "and each one's page")

(println "arena-reclaim-counters: ok")
