(elle/epoch 18)
# audited: 2026-11-24
# A step that makes a region: a new one, one the lowerer merged, or one made again beside the last.
# tools/glean/present.md

(fn []
  (def {:refuse refuse
        :emit emit
        :record record
        :with-record with-record
        :subtree subtree
        :names-of names-of} ((import "./regions")))

  (defn in-use? [state tok]
    "Whether a register, a slot or a cell holds TOK."
    (or (any? (fn [t] (= t tok)) (values (get state :regs)))
        (any? (fn [t] (= t tok)) (values (get state :slots)))
        (any? (fn [rec] (= (get rec :content) tok))
              (values (get state :regions)))))

  (defn displace [ctx state tok p]
    "STATE with the region TOK made last folded where its fate lies, before P
  makes TOK again: held where counted, into its owner where owned."
    (def rec (record state tok))
    (def toks (subtree state tok))
    (def names (names-of state toks))
    (def mode (get rec :mode))
    (def kept
      (fold (fn [s t] (put s :regions (del (get s :regions) t))) state toks))
    (cond
      (= mode :gone) kept
      (= mode :activation)
        (put kept :dues (union (get kept :dues) names))
      (= mode :counted)
        (begin
          (when (> (get rec :count) 0) (emit ctx :held names p))
          kept)
      (let [owner (get mode 1)
            orec (record kept owner)]
        (with-record kept owner
                     (put orec :carries (union (get orec :carries) names))))))

  (defn rename [state from to]
    "STATE with each reference to the region FROM made to TO instead."
    (defn swap [t]
      (if (= t from) to t))
    (defn swapped [m]
      (from-pairs (map (fn [[k t]] [k (swap t)]) (pairs m))))
    (defn moved [rec]
      (def mode (get rec :mode))
      (def owned
        (if (and (array? mode) (= (get mode 1) from))
          (put rec :mode [:owned to])
          rec))
      (if (has? owned :content)
        (put owned :content (swap (get owned :content)))
        owned))
    (merge state
           {:regs (swapped (get state :regs))
            :slots (swapped (get state :slots))
            :map (swapped (get state :map))
            :regions (from-pairs (map (fn [[t rec]] [(swap t) (moved rec)])
                                      (pairs (get state :regions))))}))

  (defn set-aside [ctx state tok p]
    "STATE with the region TOK moved to its second token, where something still
  holds it, so the step at P can make TOK again; the region there before ends."
    (def old [;tok :old])
    (def taken (record state old))
    (when (and taken (in-use? state old))
      (refuse :remade (string "the step at " p
                              " makes a region again while both of its regions are held")))
    (rename (if taken (displace ctx state old p) state) tok old))

  (defn counted [ctx rec names]
    "Answer NAMES under :counted where the function counts the region REC above
  zero."
    (when (and (= (get rec :mode) :counted) (> (get rec :count) 0))
      (each v in names
        (add (get (get ctx :out) :counted) v))))

  (defn make [ctx state p dst static count names]
    "STATE with the register DST holding the region the step at P makes, mapped
  by STATIC where STATIC is not nil, with COUNT and the value NAMES. A merged
  STATIC that maps a region reuses it, and adds NAMES to it."
    (def mapped (when static (get (get state :map) static)))
    (def reuse
      (and mapped (has? (get ctx :merged) static)
           (not= (get (record state mapped) :mode) :gone)))
    (if reuse
      (let [rec (record state mapped)]
        (counted ctx rec names)
        (put (with-record state mapped
                          (put rec :values (union (get rec :values) names)))
             :regs (put (get state :regs) dst mapped)))
      (let [tok [:made p]
            dropped (put state :regs (del (get state :regs) dst))
            placed (cond
                     (nil? (record dropped tok)) dropped
                     (in-use? dropped tok) (set-aside ctx dropped tok p)
                     (displace ctx dropped tok p))
            rec {:count count :mode :counted :values names :carries ||}
            fresh (with-record placed tok rec)
            held (put fresh :regs (put (get fresh :regs) dst tok))]
        (counted ctx rec names)
        (if static
          (put held :map (put (get held :map) static tok))
          held))))

  {:make make})
