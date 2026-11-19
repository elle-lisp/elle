(elle/epoch 15)
# audited: 2026-11-18
## The leapjoin, which extends each source tuple with the values several relations agree on, and its six leapers.
## docs/stdlib/proof/overview.md
## docs/stdlib/proof.md
##
## A port of datafrog 2.0.1, src/treefrog.rs. Datafrog is Copyright (c) Frank
## McSherry, The Rust Project Developers and the Datafrog Developers, and is
## licensed under Apache-2.0 or MIT, at your option.
##
## A leaper is a struct whose `:leaper` names its kind. For each source tuple,
## each leaper counts the values it would propose, and nil stands for a
## leaper that proposes none of its own and only removes. The leaper with
## the fewest proposes, and every other leaper removes from what it proposed.

(fn [&named relation]
  (defn argument-error [message]
    (error {:error :argument-error :message message}))

  (defn extend-with [r key]
    "A leaper that proposes, or keeps, each v with [(KEY t) v] in R."
    {:leaper :extend-with :relation r :key key})

  (defn extend-anti [r key]
    "A leaper that removes each v with [(KEY t) v] in R."
    {:leaper :extend-anti :relation r :key key})

  (defn filter-with [r pair]
    "A leaper that keeps t only when the tuple (PAIR t) is in R."
    {:leaper :filter-with :relation r :pair pair})

  (defn filter-anti [r pair]
    "A leaper that keeps t only when the tuple (PAIR t) is not in R."
    {:leaper :filter-anti :relation r :pair pair})

  (defn prefix-filter [p]
    "A leaper that keeps t only when (P t) is truthy."
    {:leaper :prefix-filter :predicate p})

  (defn value-filter [p]
    "A leaper that keeps each value v for which (P t v) is truthy."
    {:leaper :value-filter :predicate p})

  # ── what one leaper does with one tuple ───────────────────────────────
  (defn key-range [r k]
    "The indices [start end] of the tuples of the relation R whose key is K."
    (let [xs (relation:elements r)
          start (relation:binary-search xs
                                        (fn [t]
                                          (relation:before? (relation:key-of t)
                                            k)))
          end (relation:gallop xs start
                               (fn [t]
                                 (not (relation:before? k (relation:key-of t)))))]
      [start end]))

  (defn keeps? [leaper t]
    "Whether the filter LEAPER keeps the tuple T."
    (match (leaper :leaper)
      :filter-with
        (relation:contains? (leaper :relation) ((leaper :pair) t))
      :filter-anti
        (not (relation:contains? (leaper :relation) ((leaper :pair) t)))
      :prefix-filter
        (if ((leaper :predicate) t) true false)))

  (defn count [leaper t]
    "How many values LEAPER proposes for T, with the range they come from, as
     [count range]. A count of nil means the leaper proposes none of its own."
    (match (leaper :leaper)
      :extend-with
        (let [[start end] (key-range (leaper :relation) ((leaper :key) t))]
          [(- end start) [start end]])
      :extend-anti [nil nil]
      :value-filter [nil nil]
      _
        [(if (keeps? leaper t) nil 0) nil]))

  (defn values-in [leaper span]
    "The values of the tuples of LEAPER's relation in the index range SPAN."
    (let [xs (relation:elements (leaper :relation))
          [start end] span
          out @[]]
      (each i in (range start end)
        (push out (relation:value-of (get xs i))))
      out))

  (defn retain-present [values xs start end]
    "The values that the tuples of XS from START to END hold, as their value.
     VALUES and those tuples both ascend, so one gallop walks them."
    (let [out @[]
          @at start]
      (each v in values
        (assign
          at
          (relation:gallop xs at
                           (fn [t] (relation:before? (relation:value-of t) v))))
        (when (and (< at end) (relation:same? (relation:value-of (get xs at)) v))
          (push out v)))
      out))

  (defn retain-absent [values xs start end]
    "The values that no tuple of XS from START to END holds as its value."
    (let [out @[]
          @at start]
      (each v in values
        (assign
          at
          (relation:gallop xs at
                           (fn [t] (relation:before? (relation:value-of t) v))))
        (unless (and (< at end)
                     (relation:same? (relation:value-of (get xs at)) v))
          (push out v)))
      out))

  (defn intersect [leaper t span values]
    "The VALUES that LEAPER keeps for T. SPAN is the range its count found."
    (match (leaper :leaper)
      :extend-with
        (let [[start end] span]
          (retain-present values (relation:elements (leaper :relation)) start
                          end))
      :extend-anti
        (let [[start end] (key-range (leaper :relation) ((leaper :key) t))]
          (if (= start end)
            values
            (retain-absent values (relation:elements (leaper :relation)) start
                           end)))
      :value-filter
        (filter (fn [v] ((leaper :predicate) t v)) values)
      _ values))

  # ── the leapjoin ──────────────────────────────────────────────────────
  (defn leapers-of [leapers]
    "LEAPERS as an array: one leaper, or an array of them."
    (if (array? leapers) leapers [leapers]))

  (defn check-proposer [leapers]
    "Raise :argument-error unless one of LEAPERS proposes values: an
     extend-with, or a filter-with, filter-anti or prefix-filter alone."
    (let [kinds (map (fn [l] (l :leaper)) leapers)
          alone (and (= 1 (length leapers))
                     (has? |:filter-with :filter-anti :prefix-filter|
                           (get kinds 0)))]
      (unless (or alone (any? (fn [k] (= k :extend-with)) kinds))
        (argument-error "leapjoin: no leaper proposes values; one must be an extend-with, or a filter that stands alone"))))

  (defn leapjoin-tuples [source leapers f]
    "The relation of (F t v) for each tuple t of the sorted array SOURCE and
     each value v that LEAPERS agree on."
    (let [ls (leapers-of leapers)
          results @[]]
      (check-proposer ls)
      (if (and (= 1 (length ls)) (not= :extend-with ((get ls 0) :leaper)))
        # A filter alone proposes nil once for each tuple it keeps.
        (each t in source
          (when (keeps? (get ls 0) t) (push results (f t nil))))
        (each t in source
          (let [counts (map (fn [l] (count l t)) ls)
                @least nil
                @least-at nil]
            (each i in (range (length ls))
              (let [[c _] (get counts i)]
                (when (and (not (nil? c)) (or (nil? least) (< c least)))
                  (assign least c)
                  (assign least-at i))))
            (when (> least 0)
              (var values
                   (values-in (get ls least-at) (get (get counts least-at) 1)))
              (each i in (range (length ls))
                (unless (= i least-at)
                  (assign
                    values
                    (intersect (get ls i) t (get (get counts i) 1) values))))
              (each v in values
                (push results (f t v)))))))
      (relation:relation results)))

  (defn leapjoin [r leapers f]
    "The relation of (F t v) for each tuple t of the relation R and each value
     v that LEAPERS agree on."
    (leapjoin-tuples (relation:elements r) leapers f))

  {:extend-with extend-with
   :extend-anti extend-anti
   :filter-with filter-with
   :filter-anti filter-anti
   :prefix-filter prefix-filter
   :value-filter value-filter
   :leapjoin leapjoin
   :leapjoin-tuples leapjoin-tuples})
