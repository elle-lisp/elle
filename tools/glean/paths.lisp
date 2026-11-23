(elle/epoch 18)
# audited: 2026-11-23
# Glean's error paths: the points from which every path to an exit passes an error emit.
# tools/glean/census.md

(fn []
  (def {:edges edges} ((import "./points")))

  (defn error-emit? [node]
    "Whether NODE is an error emit: an Emit whose signal holds :error, or a MatchFail."
    (def kind (get node :kind))
    (or (= kind :match-fail)
        (and (= kind :emit) (contains? (get node :signal) :error))))

  (defn backward [n succs seed? stops?]
    "The points that reach a point SEED? holds over SUCCS, by no point STOPS?
  holds, as a backward fixpoint over the N points. A seed always holds."
    (def held @[])
    (each p in (range n)
      (push held (seed? p)))
    (var changed true)
    (while changed
      (assign changed false)
      (each p in (reverse (range n))
        (when (and (not (get held p)) (not (stops? p))
                   (any? (fn [q] (get held q)) (get succs p)))
          (put held p true)
          (assign changed true))))
    held)

  (defn error-paths [view]
    "Over VIEW, the set of error-only points, and the set of the first points of
  the resume blocks of each error Emit. A point is error-only when some path
  from it reaches an error emit and none reaches an exit without passing one."
    (def nodes (get view :nodes))
    (def n (length nodes))
    (def all-edges (->array (map (fn [p] (edges view p)) (range n))))
    (def succs
      (->array (map (fn [es] (filter number? (map (fn [e] (get e 1)) es)))
                    all-edges)))
    (defn emits? [p]
      (error-emit? (get (get nodes p) :node)))
    (def normal
      (backward n succs
                (fn [p]
                  (and (not (emits? p))
                       (any? (fn [e] (not (number? (get e 1))))
                             (get all-edges p)))) emits?))
    (def erring (backward n succs emits? (fn [_] false)))
    (def resumes
      (map (fn [p]
             (get (get view :starts)
                  (get (get (get nodes p) :node) :resume-label)))
           (filter (fn [p] (and (get (get nodes p) :term?) (emits? p)))
                   (range n))))
    [(set ;(filter (fn [p] (and (get erring p) (not (get normal p)))) (range n)))
     (set ;resumes)])

  {:error-paths error-paths})
