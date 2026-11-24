(elle/epoch 15)
# audited: 2026-11-18
## The iteration, a variable's three stages, the round `changed?` ends, and the rules that write a variable.
## docs/stdlib/proof/overview.md
## docs/stdlib/proof.md
##
## A port of datafrog 2.0.1, src/lib.rs, src/join.rs and src/map.rs. Datafrog
## is Copyright (c) Frank McSherry, The Rust Project Developers and the
## Datafrog Developers, and is licensed under Apache-2.0 or MIT, at your option.
##
## A variable is one mutable struct that the iteration and every rule share.
## `:to-add` holds the relations added since the last round, `:recent` the
## relation of tuples new this round, and `:stable` the batches of older
## tuples, each batch a relation.

(fn [&named relation]
  (defn argument-error [message]
    (error {:error :argument-error :message message}))

  (defn iteration []
    "A new iteration, with no variable."
    @{:variables @[]})

  (defn variable [it name &named @distinct]
    "A new variable of the iteration IT, named NAME. A variable is distinct
     unless :distinct is false."
    (default distinct true)
    (let [v @{:variable true
              :name name
              :distinct distinct
              :stable @[]
              :recent (relation:relation [])
              :to-add @[]}]
      (push (it :variables) v)
      v))

  (defn insert [v r]
    "Add the tuples of the relation R to the variable V, for the next round."
    (unless (relation:empty? r) (push (v :to-add) r)))

  (defn extend [v xs]
    "Add the tuples of the array XS to the variable V, for the next round."
    (insert v (relation:relation xs)))

  (defn recent [v]
    "The tuples new to the variable V this round, as a sorted array."
    (relation:elements (v :recent)))

  (defn not-in [xs batch]
    "The tuples of the sorted array XS that the sorted array BATCH lacks. A
     batch much longer than XS is searched by gallop, a shorter one by steps."
    (let [out @[]
          n (length batch)
          gallop? (> n (* 4 (length xs)))
          @at 0]
      (each x in xs
        (if gallop?
          (assign at (relation:gallop batch at (fn [y] (relation:before? y x))))
          (while (and (< at n) (relation:before? (get batch at) x))
            (assign at (+ at 1))))
        (unless (and (< at n) (relation:same? (get batch at) x)) (push out x)))
      (freeze out)))

  (defn variable-changed? [v]
    "End the round for the variable V: its new tuples become stable, and the
     tuples added since the last round become new. Return whether any are."
    # The new tuples join the stable batches. A batch no more than twice as
    # long as them merges in first, so the batches stay few.
    (unless (relation:empty? (v :recent))
      (let [stable (v :stable)
            @batch (v :recent)]
        (put v :recent (relation:relation []))
        (while (and (not (empty? stable))
                    (<= (length (relation:elements
                                  (get stable (- (length stable) 1))))
                        (* 2 (length (relation:elements batch)))))
          (assign batch (relation:merge batch (pop stable))))
        (push stable batch)))
    # The tuples added become new, less those a distinct variable holds.
    (let [to-add (v :to-add)]
      (unless (empty? to-add)
        (var added (pop to-add))
        (while (not (empty? to-add))
          (assign added (relation:merge added (pop to-add))))
        (when (v :distinct)
          (var xs (relation:elements added))
          (each batch in (v :stable)
            (assign xs (not-in xs (relation:elements batch))))
          (assign added (relation:from-sorted xs)))
        (put v :recent added)))
    (not (relation:empty? (v :recent))))

  (defn changed? [it]
    "End a round for every variable of the iteration IT. Return true while
     some variable holds new tuples."
    (var result false)
    (each v in (it :variables)
      (when (variable-changed? v) (assign result true)))
    result)

  (defn complete [v]
    "The tuples of the variable V as a relation, once the loop has ended. It
     raises :argument-error while V holds new tuples or tuples to add."
    (unless (and (relation:empty? (v :recent)) (empty? (v :to-add)))
      (argument-error (string "complete: the variable " (v :name)
                              " still changes; call changed? until it is false")))
    (var result (relation:relation []))
    (each batch in (v :stable)
      (assign result (relation:merge result batch)))
    result)

  # ── the rules ─────────────────────────────────────────────────────────
  (defn stages [x]
    "The new tuples and the stable batches of X, a variable or a relation. A
     relation has no new tuples, and is its own one stable batch."
    (if (relation:relation? x) [[] [x]] [(recent x) (x :stable)]))

  (defn from-join [out v x f]
    "Add (F k v1 v2) to OUT for each [k v1] of the variable V and [k v2] of
     X, a variable or a relation, where one of the two tuples is new."
    (let [results @[]
          emit (fn [k v1 v2] (push results (f k v1 v2)))
          recent1 (recent v)
          [recent2 stable2] (stages x)]
      (each batch in stable2
        (relation:join-helper recent1 (relation:elements batch) emit))
      (each batch in (v :stable)
        (relation:join-helper (relation:elements batch) recent2 emit))
      (relation:join-helper recent1 recent2 emit)
      (insert out (relation:relation results))))

  (defn from-antijoin [out v keys f]
    "Add (F k val) to OUT for each new [k val] of the variable V whose key is
     not a tuple of the relation KEYS."
    (insert out (relation:antijoin-tuples (recent v) keys f)))

  (defn from-map [out v f]
    "Add (F t) to OUT for each new tuple t of the variable V."
    (let [results @[]]
      (each t in (recent v)
        (push results (f t)))
      (insert out (relation:relation results))))

  {:iteration iteration
   :variable variable
   :insert insert
   :extend extend
   :recent recent
   :changed? changed?
   :complete complete
   :from-join from-join
   :from-antijoin from-antijoin
   :from-map from-map})
