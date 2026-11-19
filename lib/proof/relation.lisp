(elle/epoch 15)
# audited: 2026-11-18
## A relation of sorted tuples, the merge of two, the gallop and binary search, and the joins, antijoin and map.
## docs/stdlib/proof/overview.md
## docs/stdlib/proof.md
##
## A port of datafrog 2.0.1, src/lib.rs and src/join.rs. Datafrog is
## Copyright (c) Frank McSherry, The Rust Project Developers and the Datafrog
## Developers, and is licensed under Apache-2.0 or MIT, at your option.
##
## Tuples order by `compare`, so two tuples are one when they compare 0. A
## relation is a struct whose `:elements` is an immutable array, sorted, with
## no two tuples that compare 0. A join reads a tuple as `[key value]`.

(fn []
  (defn before? [a b]
    "Whether A orders before B."
    (= -1 (compare a b)))

  (defn same? [a b]
    "Whether A and B are one tuple."
    (= 0 (compare a b)))

  (defn key-of [t]
    (get t 0))

  (defn value-of [t]
    (get t 1))

  (defn from-sorted [xs]
    "The relation whose tuples are XS, sorted and distinct already."
    {:relation true :elements xs})

  (defn relation [xs]
    "The relation of the tuples of the array or list XS: sorted, one of each."
    (let [sorted (sort (->array xs))
          out @[]]
      (each t in sorted
        (when (or (empty? out) (not (same? (get out (- (length out) 1)) t)))
          (push out t)))
      (from-sorted (freeze out))))

  (defn relation? [x]
    "Whether X is a relation."
    (and (struct? x) (= true (get x :relation))))

  (defn elements [r]
    "The tuples of the relation R, as an immutable array in the order."
    (r :elements))

  (defn empty-relation? [r]
    (empty? (r :elements)))

  # ── the gallop and the binary search ──────────────────────────────────
  (defn gallop [xs start holds?]
    "The first index from START at which HOLDS? fails, for a HOLDS? that
     holds on a prefix of XS from START. It steps by doubling, then halving."
    (let [n (length xs)
          @lo start]
      (when (and (< lo n) (holds? (get xs lo)))
        (var step 1)
        (while (and (< (+ lo step) n) (holds? (get xs (+ lo step))))
          (assign lo (+ lo step))
          (assign step (* step 2)))
        (assign step (bit/shr step 1))
        (while (> step 0)
          (when (and (< (+ lo step) n) (holds? (get xs (+ lo step))))
            (assign lo (+ lo step)))
          (assign step (bit/shr step 1)))
        (assign lo (+ lo 1)))
      lo))

  (defn binary-search [xs holds?]
    "The first index of XS at which HOLDS? fails, for a HOLDS? that holds on
     a prefix of XS."
    (let [@lo 0
          @hi (length xs)]
      (while (< lo hi)
        (let [mid (+ lo (bit/shr (- hi lo) 1))]
          (if (holds? (get xs mid)) (assign lo (+ mid 1)) (assign hi mid))))
      lo))

  (defn contains? [r t]
    "Whether the relation R holds the tuple T."
    (let [xs (r :elements)
          i (binary-search xs (fn [x] (before? x t)))]
      (and (< i (length xs)) (same? (get xs i) t))))

  # ── merge ─────────────────────────────────────────────────────────────
  (defn merge [r1 r2]
    "The union of the relations R1 and R2."
    (var xs (r1 :elements))
    (var ys (r2 :elements))
    (cond
      (empty? xs) r2
      (empty? ys) r1
      true
        (begin
          # xs starts with the lower tuple.
          (when (before? (get ys 0) (get xs 0))
            (let [t xs]
              (assign xs ys)
              (assign ys t)))
          (if (before? (get xs (- (length xs) 1)) (get ys 0))
            (from-sorted (concat xs ys))
            (let [out @[]
                  ny (length ys)
                  @j 0]
              (push out (get xs 0))
              (when (same? (get xs 0) (get ys 0)) (assign j 1))
              (each i in (range 1 (length xs))
                (let [x (get xs i)]
                  (while (and (< j ny) (before? (get ys j) x))
                    (push out (get ys j))
                    (assign j (+ j 1)))
                  (when (and (< j ny) (same? (get ys j) x)) (assign j (+ j 1)))
                  (push out x)))
              (while (< j ny)
                (push out (get ys j))
                (assign j (+ j 1)))
              (from-sorted (freeze out)))))))

  # ── the joins ─────────────────────────────────────────────────────────
  (defn join-helper [xs ys emit]
    "Call (EMIT k v1 v2) for each [k v1] of XS and [k v2] of YS whose keys
     compare 0, where XS and YS are sorted arrays of [key value] tuples."
    (let [nx (length xs)
          ny (length ys)
          @i 0
          @j 0]
      (while (and (< i nx) (< j ny))
        (let [kx (key-of (get xs i))
              ky (key-of (get ys j))
              c (compare kx ky)]
          (cond
            (= c -1)
              (assign i (gallop xs i (fn [t] (before? (key-of t) ky))))
            (= c 1)
              (assign j (gallop ys j (fn [t] (before? (key-of t) kx))))
            true
              (let [@ci i
                    @cj j]
                (while (and (< ci nx) (same? (key-of (get xs ci)) kx))
                  (assign ci (+ ci 1)))
                (while (and (< cj ny) (same? (key-of (get ys cj)) ky))
                  (assign cj (+ cj 1)))
                (each a in (range i ci)
                  (each b in (range j cj)
                    (emit kx (value-of (get xs a)) (value-of (get ys b)))))
                (assign i ci)
                (assign j cj)))))))

  (defn join [r1 r2 f]
    "The relation of (F k v1 v2) for each [k v1] of R1 and [k v2] of R2."
    (let [out @[]]
      (join-helper (r1 :elements) (r2 :elements)
                   (fn [k v1 v2] (push out (f k v1 v2))))
      (relation out)))

  (defn antijoin-tuples [xs keys f]
    "The relation of (F k v) for each [k v] of the sorted array XS whose key is
     not a tuple of the relation KEYS."
    (let [ks (keys :elements)
          out @[]
          @at 0]
      (each t in xs
        (let [k (key-of t)]
          (assign at (gallop ks at (fn [x] (before? x k))))
          (unless (and (< at (length ks)) (same? (get ks at) k))
            (push out (f k (value-of t))))))
      (relation out)))

  (defn antijoin [r keys f]
    "The relation of (F k v) for each [k v] of R whose key is not a tuple of
     the relation KEYS."
    (antijoin-tuples (r :elements) keys f))

  (defn map-relation [r f]
    "The relation of (F t) for each tuple t of R."
    (let [out @[]]
      (each t in (r :elements)
        (push out (f t)))
      (relation out)))

  {:relation relation
   :relation? relation?
   :from-sorted from-sorted
   :elements elements
   :empty? empty-relation?
   :before? before?
   :same? same?
   :key-of key-of
   :value-of value-of
   :gallop gallop
   :binary-search binary-search
   :contains? contains?
   :merge merge
   :join-helper join-helper
   :join join
   :antijoin-tuples antijoin-tuples
   :antijoin antijoin
   :map map-relation})
