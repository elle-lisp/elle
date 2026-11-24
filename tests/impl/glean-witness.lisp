(elle/epoch 18)
# audited: 2026-11-24
# A witness runs a function in a child, and reads a leak, a use after free and a hold that grows.
# tools/glean/witness.md
#
# Each witnessed shape stands beside one that keeps nothing. keep grows a sink
# on every call; drop makes an array and drops it. seed is the reader of
# #1505, which faults under guardfree on this implementation, and panics first
# on a build with debug assertions. hold keeps n pairs to its return; churn
# drops each array it makes. The counter-factual: a witness that ran its early
# child without guardfree would read seed as clean, and one that took only the
# fault would read the panic as an error.
# One that took any reading as a witness would read drop and churn as leaks,
# one that took none would miss keep, and one that read a failed child's
# output would miss bad's refusal. One that built each argument list once
# would read fill's growing argument as a leak.

(def {:witness witness} ((import "../../tools/glean/witness")))
(def elle (elle/executable))

(defn entry [source name & more]
  "A table entry for the function NAME of SOURCE, MORE over it."
  (merge {:document "witness" :name name :source source :args ["[1]"]}
         (struct ;more)))

(def keep (entry "(def sink @[])
(defn keep [x] (push sink @[x]) nil)" "keep"))
(def drop (entry "(defn drop [x] (length @[x]))" "drop"))
(def bad
  (entry "(defn bad [x] (error {:error :refused :message \"no\"}))" "bad"))

(assert (= (witness elle keep [:none]) {:none :witnessed})
        "a call that grows a sink leaves objects behind")
(assert (= (witness elle drop [:none]) {:none :unwitnessed})
        "a call that drops what it makes leaves none")
(assert (= (witness elle bad [:none :early]) {:none :error :early :error})
        "a function that refuses its arguments is no witness")
(def fill (entry "(defn fill [acc] (push acc @[1]) nil)" "fill" :args ["[@[]]"]))
(assert (= (witness elle fill [:none]) {:none :unwitnessed})
        "each call stores into an argument list of its own")
(def named-f (entry "(defn f [x] (length @[x]))" "f"))
(assert (= (witness elle named-f [:none :early])
           {:none :unwitnessed :early :unwitnessed})
        "a function may bear any name the driver binds")

(def seed
  (entry "(defn seed []
  (with-temp-dir d
    (let [p (path/join d \"f\")]
      (file/write p \"hello\\nworld\\n\")
      (let [port (port/open p :read)]
        (let [line (port/read-line port)]
          (let [f (fn [] (length line))] (f))
          (ev/join (ev/spawn (fn [] 42))))))))"
         "seed" :args ["[]"]))

(assert (= (witness elle seed [:early]) {:early :witnessed})
        "a captured resume value read after its free faults")
(assert (= (witness elle drop [:early]) {:early :unwitnessed})
        "a function with no use after free does not")

(def hold
  (entry "(defn hold [n]
  (def @xs ())
  (def @i 0)
  (while (< i n)
    (assign xs (pair @[i] xs))
    (assign i (+ i 1)))
  (length xs))"
         "hold" :sized "(fn [n] [n])"))
(def churn
  (entry "(defn churn [n]
  (def @i 0)
  (while (< i n)
    (length @[i])
    (assign i (+ i 1)))
  n)"
         "churn" :sized "(fn [n] [n])"))

(assert (= (witness elle hold [:late]) {:late :witnessed})
        "a call that holds a pair per element climbs with its size")
(assert (= (witness elle churn [:late]) {:late :unwitnessed})
        "a call that drops each array stays flat")
(assert (= (witness elle drop [:late]) {:late :unsized})
        "an entry with no size has no late witness")

(println "glean-witness: ok")
