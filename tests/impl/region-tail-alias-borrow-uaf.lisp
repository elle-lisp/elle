(elle/epoch 15)
# audited: 2026-10-13
# A tail call that hands over a binding renaming a borrowed value mints the reference it moves.
# docs/impl/region/rules.md
#
# The sidecar arms guardfree, so a read of a freed page faults.
#
# Each subject passes a tail call a `let`, `letrec`, `def` or `assign` binding
# whose value is borrowed: a pattern leaf, a captured upvalue, or the version a
# `default` builds over a `&named` parameter. The callee releases each
# parameter it owns, so the caller must hand it a reference of its own.
#
# The counter-factual: a reading of the alias's own binding finds no capture
# and no pattern, so the call moves a reference the frame never held. The
# callee's release then lowers a count some other holder owns, and the value
# comes back freed. A closure answers garbage, raises the wrong arity, or
# panics the VM on a code object that is now a fiber.
#
# The rates guard the other direction. A mint for an alias that owns its
# reference, or for the fresh arm of a `default`, leaks once per call.

(defn inner [s f]
  f)
(defn inner-one [f]
  f)
(defn keep [&named store f]
  {:f f :store store})

(defn forward-defaulted [&named s @f]
  "Default a &named closure, then forward it beside another &named value."
  (default f (fn [x] [:default x]))
  (inner s f))

(defn forward-named [&named @store @f]
  "Default a &named closure, then forward both by name to a collector."
  (default f (fn [x] [:default x]))
  (keep :store store :f f))

(defn forward-reassigned [&named s @f]
  "Reassign a &named closure to itself, then forward it."
  (assign f f)
  (inner s f))

(defn forward-struct-leaf [t]
  "Rename a struct pattern's leaf, then forward it beside a sibling leaf."
  (let [{:s s :f f} t
        g f]
    (inner s g)))

(defn forward-array-leaf [t]
  "Rename an array pattern's leaf in an inner let, then forward it."
  (let [[s f] t]
    (let [g f]
      (inner s g))))

(defn forward-def [&named s f]
  "Rename a &named closure with def, then forward it."
  (def g f)
  (inner s g))

(defn forward-letrec [&named s f]
  "Rename a &named closure with letrec, then forward it."
  (letrec [g f]
    (inner s g)))

(defn forward-chain [&named s f]
  "Rename a &named closure twice, then forward the second name."
  (let [g f
        h g]
    (inner s h)))

(defn capture-alias [f]
  "Answer a closure that renames its capture and forwards the new name."
  (fn []
    (let [g f]
      (inner-one g))))

(defn counted-read-alias []
  "Read a reassigned captured binding into a new name, then forward it."
  (def @x (list 1 2))
  (let [h (fn []
            (let [g x]
              (length g)))]
    (assign x (list 3 4))
    (h)))

# ── each alias still names the caller's closure ──────────────────
# A fresh closure per call, as the caller writes it: its region holds no
# reference but the caller's and the one each container takes.
(def @i 0)
(while (%lt i 200)
  (assert (= ((forward-defaulted :s i :f (fn [x] [:given x])) 1) [:given 1])
          "a defaulted &named closure came back freed from a tail call")
  (assert (= ((forward-defaulted :s i) 1) [:default 1])
          "the default closure came back freed from a tail call")
  (assert (= ((get (forward-named :store i :f (fn [x] [:given x])) :f) 1)
             [:given 1])
          "a defaulted &named closure forwarded by name came back freed")
  (assert (= ((forward-reassigned :s i :f (fn [x] [:given x])) 1) [:given 1])
          "a reassigned &named closure came back freed from a tail call")
  (assert (= ((forward-struct-leaf {:s i :f (fn [x] [:given x])}) 1) [:given 1])
          "a renamed struct leaf came back freed from a tail call")
  (assert (= ((forward-array-leaf [i (fn [x] [:given x])]) 1) [:given 1])
          "a renamed array leaf came back freed from a tail call")
  (assert (= ((forward-def :s i :f (fn [x] [:given x])) 1) [:given 1])
          "a def alias came back freed from a tail call")
  (assert (= ((forward-letrec :s i :f (fn [x] [:given x])) 1) [:given 1])
          "a letrec alias came back freed from a tail call")
  (assert (= ((forward-chain :s i :f (fn [x] [:given x])) 1) [:given 1])
          "an alias of an alias came back freed from a tail call")
  (assign i (%add i 1)))

# ── a renamed capture survives every call that forwards it ───────
# The closure env holds the capture's one reference, so each call that moves
# it without a mint lowers that count, and a later call reads a freed page.
(def h (capture-alias (fn [x] [:given x])))
(def @j 0)
(while (%lt j 200)
  (assert (= ((h) 1) [:given 1]) "a renamed capture came back freed")
  (assign j (%add j 1)))

(assert (= (counted-read-alias) 2) "a counted read forwarded by a tail call")

# ── no subject leaks a region per call ───────────────────────────
(defn region-delta [f iters]
  (def before (arena/region-count))
  (def @k 0)
  (while (%lt k iters)
    (f k)
    (assign k (%add k 1)))
  (%sub (arena/region-count) before))

# One closure for every call, so a rate counts what the call leaks rather
# than the closure the caller built for it.
(def given (fn [x] [:given x]))

(let [given-arm (region-delta (fn [k] (forward-defaulted :s k :f given)) 200)
      default-arm (region-delta (fn [k] (forward-defaulted :s k)) 200)
      leaf (region-delta (fn [k] (forward-struct-leaf {:s k :f given})) 200)
      capture (region-delta (fn [k] (h)) 200)
      counted (region-delta (fn [k] (counted-read-alias)) 200)]
  (assert (%lt given-arm 20)
          (concat "the given arm of a default leaked: delta="
                  (number->string given-arm)))
  (assert (%lt default-arm 20)
          (concat "the default arm of a default leaked: delta="
                  (number->string default-arm)))
  (assert (%lt leaf 20)
          (concat "a renamed struct leaf leaked: delta=" (number->string leaf)))
  (assert (%lt capture 20)
          (concat "a renamed capture leaked: delta=" (number->string capture)))
  (assert (%lt counted 20)
          (concat "a counted read alias leaked: delta=" (number->string counted))))

(println "region-tail-alias-borrow-uaf: ok")
