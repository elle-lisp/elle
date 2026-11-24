(elle/epoch 18)
# audited: 2026-11-24
# The regions present walks: each one's count, mode and names, and the steps that raise, lower, adopt and release them.
# tools/glean/present.md

# A state is {:regs :slots :map :regions :dues}: the region of each register
# and slot, the region each static region maps, a record per region, and the
# names the activation will release at its exits. A region is a token. Its
# record holds :count, :mode (:counted, [:owned token], :activation or :gone),
# :values, :carries (the names of owned regions folded into it) and, for a
# cell, :content. :none stands for no region.

(fn []
  (defn refuse [reason message]
    "Refuse the function, for REASON."
    (error {:error :present-error
            :reason reason
            :message (string "present: " message)}))

  (defn emit [ctx key names p]
    "Answer [v P] under KEY for each of NAMES."
    (def out (get (get ctx :out) key))
    (each v in names
      (add out [v p])))

  (defn record [state tok]
    (get (get state :regions) tok))

  (defn with-record [state tok rec]
    (put state :regions (put (get state :regions) tok rec)))

  (defn content [state tok]
    "The region a value of TOK reads as, a cell's content for a cell."
    (def rec (if (= tok :none) nil (record state tok)))
    (if (and (not (nil? rec)) (has? rec :content)) (get rec :content) tok))

  (defn subtree [state tok]
    "TOK and each region it owns, to any depth."
    (def owned
      (->array (filter (fn [t] (= (get (record state t) :mode) [:owned tok]))
                       (keys (get state :regions)))))
    [tok ;(mapcat (fn [t] (subtree state t)) owned)])

  (defn names-of [state toks]
    "The names the regions TOKS carry, folded regions included."
    (fold (fn [acc t]
            (def rec (record state t))
            (union (union acc (get rec :values)) (get rec :carries))) || toks))

  (defn gone [state toks]
    "STATE with each region of TOKS gone."
    (fold (fn [s t] (with-record s t (put (record s t) :mode :gone))) state toks))

  (defn release [ctx state tok p]
    "Release TOK's region at P, with each region it owns."
    (def toks (subtree state tok))
    (emit ctx :released (names-of state toks) p)
    (gone state toks))

  (defn lower [ctx state tok p how]
    "Lower TOK's count at P, as a release or, for HOW :hand, a hand-over."
    (def rec (if (= tok :none) nil (record state tok)))
    (def mode (when rec (get rec :mode)))
    (cond
      (nil? rec) state
      (= mode :gone)
        (begin
          (emit ctx :over (get rec :values) p)
          state)
      (not= mode :counted) state
      (= 0 (get rec :count))
        (begin
          (emit ctx :over (get rec :values) p)
          state)
      (> (get rec :count) 1)
        (with-record state tok (put rec :count (- (get rec :count) 1)))
      (= how :release) (release ctx state tok p)
      (let [toks (subtree state tok)]
        (emit ctx :handed (names-of state toks) p)
        (gone state toks))))

  (defn raise [ctx state tok p]
    "Raise TOK's count at P, refusing a count past the function's bound."
    (def rec (if (= tok :none) nil (record state tok)))
    (if (or (nil? rec) (not= (get rec :mode) :counted))
      state
      (let [n (+ 1 (get rec :count))]
        (when (> n (get ctx :bound))
          (refuse :unbounded (string "a count rises past " (get ctx :bound)
                                     " at " p)))
        (with-record state tok (put rec :count n)))))

  (defn adopt [state tok owner]
    "STATE with TOK's counted region owned by OWNER, a token or :activation."
    (def rec (if (= tok :none) nil (record state tok)))
    (if (or (nil? rec) (not= (get rec :mode) :counted))
      state
      (with-record state tok
                   (merge rec
                          {:count 0
                           :mode (if (= owner :activation)
                                   :activation
                                   [:owned owner])}))))

  (defn free-group [ctx state tok p]
    "Release TOK's region at P whatever its count, as a group free does."
    (def rec (if (= tok :none) nil (record state tok)))
    (cond
      (nil? rec) state
      (= (get rec :mode) :gone)
        (begin
          (emit ctx :over (get rec :values) p)
          state)
      (release ctx state tok p)))

  (defn exit [ctx state p]
    "Answer the exit at P: each counted region above zero is held, and each
  region the activation owns is released."
    (each [tok rec] in (pairs (get state :regions))
      (def mode (get rec :mode))
      (cond
        (and (= mode :counted) (> (get rec :count) 0))
          (emit ctx :held (names-of state (subtree state tok)) p)
        (= mode :activation)
          (emit ctx :released (names-of state (subtree state tok)) p)))
    (emit ctx :released (get state :dues) p))

  {:refuse refuse
   :emit emit
   :record record
   :with-record with-record
   :content content
   :subtree subtree
   :names-of names-of
   :release release
   :lower lower
   :raise raise
   :adopt adopt
   :free-group free-group
   :exit exit})
