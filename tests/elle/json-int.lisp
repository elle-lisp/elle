## JSON integer type preservation tests

# Basic integer parsing
(def v (json/parse "42"))
(assert (int? v) "json/parse 42 is int")
(assert (= v 42) "json/parse 42 value")

# Negative integer
(def v (json/parse "-17"))
(assert (int? v) "json/parse -17 is int")
(assert (= v -17) "json/parse -17 value")

# Zero
(def v (json/parse "0"))
(assert (int? v) "json/parse 0 is int")
(assert (= v 0) "json/parse 0 value")

# Large integer
(def v (json/parse "140737488355327"))
(assert (int? v) "json/parse large int is int")
(assert (= v 140737488355327) "json/parse large int value")

# Integer in object
(def obj (json/parse "{\"x\": 42}" :keys :keyword))
(def v (get obj :x))
(assert (int? v) "json object int field is int")
(assert (= v 42) "json object int field value")

# Integer in array
(def lst (json/parse "[1, 2, 3]"))
(assert (int? (first lst)) "json array first is int")
(assert (int? (first (rest lst))) "json array second is int")

# Float stays float
(def v (json/parse "3.14"))
(assert (float? v) "json/parse 3.14 is float")

# Integer with exponent becomes float (per JSON spec)
(def v (json/parse "1e2"))
(assert (float? v) "json/parse 1e2 is float")

# Nested object integer
(def obj (json/parse "{\"a\": {\"b\": 99}}" :keys :keyword))
(def inner (get obj :a))
(def v (get inner :b))
(assert (int? v) "json nested int is int")
(assert (= v 99) "json nested int value")

# Not-float after arithmetic
(def v (json/parse "10"))
(def r (+ v 5))
(assert (int? r) "json int + int is int")
(assert (= r 15) "json int + int value")

(println "all json-int tests passed")
