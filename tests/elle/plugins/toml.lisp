(elle/epoch 8)

## TOML plugin integration tests

## Try to load the toml plugin. If it fails, exit cleanly.
(def [ok? plugin] (protect (import-file "target/release/libelle_toml.so")))
(when (not ok?)
  (print "SKIP: toml plugin not built\n")
  (exit 0))

## Extract plugin functions from the returned struct
(def parse-fn  (get plugin :parse))
(def encode-fn (get plugin :encode))

## ── toml/parse: simple nested table ────────────────────────────

(def result (parse-fn "[package]\nname = \"hello\"\nversion = \"1.0.0\""))

(assert (= (get (get result :package) :name) "hello") "toml/parse name")

(assert (= (get (get result :package) :version) "1.0.0") "toml/parse version")

## ── toml/parse: scalar types ────────────────────────────────────

(def types (parse-fn "i = 42\nf = 3.14\nb = true\ns = \"hi\""))

(assert (= (get types :i) 42) "toml/parse int")

(assert (> (get types :f) 3.0) "toml/parse float")

(assert (= (get types :b) true) "toml/parse bool")

(assert (= (get types :s) "hi") "toml/parse string")

## ── toml/parse: array ───────────────────────────────────────────

(def arr (parse-fn "a = [1, 2, 3]"))

(assert (= (length (get arr :a)) 3) "toml/parse array length")

## ── toml/encode roundtrip ───────────────────────────────────────

(def original {:name "test" :version 1})
(def encoded (encode-fn original))

(assert (string? encoded) "toml/encode returns string")

(def reparsed (parse-fn encoded))

(assert (= (get reparsed :name) "test") "toml roundtrip name")

(assert (= (get reparsed :version) 1) "toml roundtrip version")

## ── error: parse invalid TOML ───────────────────────────────────

(let [[ok? err] (protect ((fn () (parse-fn "not [valid toml"))))] (assert (not ok?) "toml/parse invalid") (assert (= (get err :error) :toml-error) "toml/parse invalid"))

## ── error: encode nil value ─────────────────────────────────────

(let [[ok? err] (protect ((fn () (encode-fn {:key nil}))))] (assert (not ok?) "toml/encode nil value") (assert (= (get err :error) :toml-error) "toml/encode nil value"))

## ── error: wrong type to parse ──────────────────────────────────

(let [[ok? _] (protect ((fn () (parse-fn 42))))] (assert (not ok?) "toml/parse wrong type"))
