## JSON roundtrip: serialize then parse should preserve int type

# int → json → int
(def v 42)
(def json-str (json/serialize v))
(assert (= json-str "42") "serialize int has no decimal")
(def v2 (json/parse json-str))
(assert (int? v2) "roundtrip int stays int")
(assert (= v2 42) "roundtrip int value")

# struct with int field roundtrip
(def obj (@struct :x 99 :y 7))
(def json-str (json/serialize obj))
(def obj2 (json/parse json-str :keys :keyword))
(def x (get obj2 :x))
(assert (int? x) "roundtrip struct int field is int")
(assert (= x 99) "roundtrip struct int field value")

# float → json → float (should NOT lose decimal)
(def v 1.0)
(def json-str (json/serialize v))
(def v2 (json/parse json-str))
(assert (float? v2) "roundtrip float stays float")

# division result through json
(def v (/ 10 3))
(def json-str (json/serialize v))
(def v2 (json/parse json-str))
(assert (float? v2) "division result through json is float")

(println "all json-roundtrip tests passed")
