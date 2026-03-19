(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

## TOML plugin integration tests

## Try to load the toml plugin. If it fails, exit cleanly.
(def [ok? plugin] (protect (import-file "target/release/libelle_toml.so")))
(when (not ok?)
  (display "SKIP: toml plugin not built\n")
  (exit 0))

## Extract plugin functions from the returned struct
(def parse-fn  (get plugin :parse))
(def encode-fn (get plugin :encode))

## ── toml/parse: simple nested table ────────────────────────────

(def result (parse-fn "[package]\nname = \"hello\"\nversion = \"1.0.0\""))

(assert-eq
  (get (get result :package) :name)
  "hello"
  "toml/parse name")

(assert-eq
  (get (get result :package) :version)
  "1.0.0"
  "toml/parse version")

## ── toml/parse: scalar types ────────────────────────────────────

(def types (parse-fn "i = 42\nf = 3.14\nb = true\ns = \"hi\""))

(assert-eq
  (get types :i)
  42
  "toml/parse int")

(assert-true
  (> (get types :f) 3.0)
  "toml/parse float")

(assert-eq
  (get types :b)
  true
  "toml/parse bool")

(assert-eq
  (get types :s)
  "hi"
  "toml/parse string")

## ── toml/parse: array ───────────────────────────────────────────

(def arr (parse-fn "a = [1, 2, 3]"))

(assert-eq
  (length (get arr :a))
  3
  "toml/parse array length")

## ── toml/encode roundtrip ───────────────────────────────────────

(def original {:name "test" :version 1})
(def encoded (encode-fn original))

(assert-true
  (string? encoded)
  "toml/encode returns string")

(def reparsed (parse-fn encoded))

(assert-eq
  (get reparsed :name)
  "test"
  "toml roundtrip name")

(assert-eq
  (get reparsed :version)
  1
  "toml roundtrip version")

## ── error: parse invalid TOML ───────────────────────────────────

(assert-err-kind
  (fn () (parse-fn "not [valid toml"))
  :toml-error
  "toml/parse invalid")

## ── error: encode nil value ─────────────────────────────────────

(assert-err-kind
  (fn () (encode-fn {:key nil}))
  :toml-error
  "toml/encode nil value")

## ── error: wrong type to parse ──────────────────────────────────

(assert-err
  (fn () (parse-fn 42))
  "toml/parse wrong type")
