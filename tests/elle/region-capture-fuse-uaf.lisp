(elle/epoch 12)
# Guardfree soundness of fusing a CAPTURING lambda (docs/impl/dissolution.md
# § "Captures").
#
# A capturing literal's body is spliced where the call stood, so the loop reads the
# captured binding directly instead of through a closure environment. That puts a
# heap value on a path no other fused shape takes: it is owned by the ENCLOSING
# frame, read once per element inside the loop, and — where the body hands it on —
# carried into an accumulator that outlives the loop. The enclosing frame must still
# own it after the walk, and the accumulator must not free it. This fixture drives
# every role with heap values, so an over-free faults at the exact access under
# --trace=guardfree rather than leaking silently. The plain-VM run also asserts the
# values, so a miscompile is loud either way.
#
# The bodies use single-primitive calls (`string`, `get`, `push`): a variadic
# comparison like `>` routes through `apply` and is not reorder-safe, which would
# decline a composition and gauge nothing there.

# The captured heap string is read once per element and concatenated into a fresh
# one, so the capture is live across the whole walk and the results outlive it.
(def tagged
  (let [tag "pfx-"]
    (map (fn [s] (string tag s)) ["aa" "bb" "cc"])))
(assert (= (->list tagged) (list "pfx-aa" "pfx-bb" "pfx-cc"))
        "a captured heap string is read per element")

# The captured value itself is handed INTO the accumulator, so a value the enclosing
# frame owns enters a structure that outlives the loop — and the frame must still own
# it. Reading the capture after the walk is the gauge.
(def kept
  (let [sentinel "keep-me"]
    (let [out (map (fn [s] (if (empty? s) sentinel s)) ["" "qq" ""])]
      (assert (= (get out 0) sentinel)
              "the captured value entered the accumulator")
      (assert (= sentinel "keep-me")
              "the enclosing frame still owns the captured value after the walk")
      out)))
(assert (= (->list kept) (list "keep-me" "qq" "keep-me"))
        "the accumulator's captured elements stay live past the loop")

# A captured heap STRUCT: the body dereferences a field of it per element, so a read
# out of the capture rides every element statement beside a fresh aggregate.
(def rich
  (let [cfg {:unit "cm"}]
    (map (fn [n] {:n n :unit (get cfg :unit)}) [1 2])))
(assert (= (get (get rich 1) :unit) "cm")
        "a field read out of a captured struct survives the walk")

# A captured MUTABLE array the body pushes into: the loop mutates a structure the
# enclosing frame owns, once per element, and the frame reads it afterwards.
(def drained
  (let [sink @[]]
    (map (fn [s]
           (push sink s)
           (length sink)) ["x" "yy" "zzz"])
    (->list sink)))
(assert (= drained (list "x" "yy" "zzz"))
        "a captured mutable sink collects the base's own heap elements")

# A captured mutable BINDING (a cell) written per element: the spliced read unwraps
# the cell exactly as the closure's did, so the running value is visible to the next
# element and the last one survives the loop.
(def joined
  (let [@acc ""]
    (map (fn [s] (assign acc (string acc s))) ["a" "bb" "c"])
    acc))
(assert (= joined "abbc")
        "a captured cell accumulates heap strings across the fused walk")

# Each terminal reads a capture the same way: the scalar answer may BE the captured
# value (a `find` hands out an element, a `fold` an accumulator seeded from one).
(def folded
  (let [sep "-"]
    (fold (fn [a x] (string a sep x)) "s" ["p" "q"])))
(assert (= folded "s-p-q") "a capturing fold combinator over heap values")
(def found
  (let [want "qq"]
    (find (fn [s] (= s want)) ["aa" "qq" "rr"])))
(assert (= found "qq") "a capturing find hands out a base element")

# A `mapcat` whose function builds its per-element array FROM the capture: the
# captured value enters an array that dies before the next base element, and the
# accumulator keeps a reference read out of it.
(def fanned
  (let [pad "_"]
    (mapcat (fn [s] [s pad]) ["a" "bb"])))
(assert (= (->list fanned) (list "a" "_" "bb" "_"))
        "a captured value read out of a per-element array outlives it")

# The base array is bound to a Var and read AFTER the walk, so the fused loop must
# neither consume nor free the base while also holding the capture.
(def base ["p" "qq" "rrr"])
(def viacapture
  (let [tag "t"]
    (map (fn [s] (string tag s)) base)))
(assert (= (length viacapture) 3) "Var-base map with a capture")
(assert (= (get base 0) "p") "the base Var survives the fused capturing walk")

# The declined shapes run the stdlib op with the capture still live: a composition
# (the reorder gate) and a self-reference. Both must leave the captured value owned.
(def staged
  (let [tag "s"]
    (let [out (map (fn [t] (string t "!"))
                   (map (fn [s] (string tag s)) ["a" "b"]))]
      (assert (= tag "s") "the capture survives the declined composition")
      out)))
(assert (= (->list staged) (list "sa!" "sb!"))
        "a declined capturing composition computes the staged value")

# Loop the whole thing so repeated fused mints/frees exercise region-id churn: a
# stale capture or accumulator region would be reused and fault on a later pass.
(def @total 0)
(def @i 0)
(while (< i 50)
  (let [tag (string "t" i)]
    (let [r (map (fn [s] (string tag s)) ["a" "bb" "c"])]
      (assign total (+ total (length r)))
      (assert (= (length tag) (length tag))
              "the capture is readable after the walk")))
  (assign i (+ i 1)))
(assert (= total 150)
        "repeated fused capturing walks stay sound under region churn")

(println "region-capture-fuse-uaf: ok")
