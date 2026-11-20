(elle/epoch 18)
# audited: 2026-11-20
## The generators of std/check, each a function of `draw` that builds its value from its choices, and `assume`.
## docs/stdlib/check.md
##
## Choice 0 is the simplest answer to every draw, so each generator makes its
## simplest value from choice 0, and a smaller choice makes a simpler value.

(fn []
  (def limit (bit/shl 1 62))

  (defn argument-error [message]
    (error {:error :argument-error :message message}))

  (defn constant [v]
    "A generator of V alone. It draws nothing."
    (fn [draw] v))

  (defn bool []
    "A generator of false, from choice 0, and true."
    (fn [draw] (= 1 (draw 2))))

  (defn int-of-choice [lo hi c]
    "The integer from LO to HI that choice C names. Choice 0 names the integer
     nearest 0. A range across 0 then alternates, a positive value before its
     negative, and runs on up the longer side once the shorter one ends."
    (cond
      (>= lo 0) (+ lo c)
      (<= hi 0) (- hi c)
      true
        (let [shorter (if (< hi (- lo)) hi (- lo))]
          (cond
            (<= c (* 2 shorter))
              (if (odd? c) (+ (bit/shr c 1) 1) (- (bit/shr c 1)))
            (> hi (- lo)) (- c shorter)
            true (- shorter c)))))

  (defn int [lo hi]
    "A generator of an integer from LO to HI, both included."
    (unless (and (int? lo) (int? hi) (<= lo hi))
      (argument-error (string "int: expected two integers lo <= hi, got " lo
                              " and " hi)))
    (let [bound (+ (- hi lo) 1)]
      (unless (and (<= 1 bound) (<= bound limit))
        (argument-error (string "int: the range from " lo " to " hi
                                " holds more than 2^62 integers")))
      (fn [draw] (int-of-choice lo hi (draw bound)))))

  (defn element [xs]
    "A generator of an element of the array XS, the first from choice 0."
    (when (empty? xs) (argument-error "element: expected a non-empty array"))
    (fn [draw] (get xs (draw (length xs)))))

  (defn one-of [gens]
    "A generator of a value of one generator in the array GENS. It draws the
     index of the generator first, so choice 0 picks the first."
    (when (empty? gens)
      (argument-error "one-of: expected a non-empty array of generators"))
    (fn [draw] ((get gens (draw (length gens))) draw)))

  (defn array-of [g most &named @min]
    "A generator of an array of MIN to MOST values of G; MIN is 0 by default.
     It draws the array as (draw G MIN MOST), so the shrinker can delete one
     element."
    (default min 0)
    (unless (and (int? min) (int? most) (<= 0 min) (<= min most))
      (argument-error (string "array-of: expected 0 <= min <= max, got " min
                              " and " most)))
    (fn [draw] (draw g min most)))

  (defn tuple [& gens]
    "A generator of an array of one value of each generator, drawn in order."
    (fn [draw]
      (let [out @[]]
        (each g in gens
          (push out (g draw)))
        (freeze out))))

  (defn map-of [f g]
    "A generator of (F v) for each value v of G."
    (fn [draw] (f (g draw))))

  (defn bind [g f]
    "A generator of a value of the generator (F v), for each value v of G."
    (fn [draw] ((f (g draw)) draw)))

  (defn assume [ok]
    "True when OK is true. When OK is false or nil, skip the case: raise
     :check-skip, which `run` reads as a case that neither passes nor fails."
    (unless ok (error {:error :check-skip :message "check: the case skips"}))
    true)

  (defn filter-of [g pred]
    "A generator of each value v of G for which (PRED v) holds. A value PRED
     rejects skips the case, so the filter never draws again."
    (fn [draw]
      (let [v (g draw)]
        (assume (pred v))
        v)))

  {:constant constant
   :bool bool
   :int int
   :element element
   :one-of one-of
   :array-of array-of
   :tuple tuple
   :map map-of
   :bind bind
   :filter filter-of
   :assume assume})
