(elle/epoch 9)

## YAML plugin integration tests

## Try to load the yaml plugin. If it fails, exit cleanly.
(def [ok? plugin] (protect (import-file "target/release/libelle_yaml.so")))
(when (not ok?)
  (print "SKIP: yaml plugin not built\n")
  (exit 0))

## Extract plugin functions from the returned struct
(def parse-fn (get plugin :parse))
(def parse-all-fn (get plugin :parse-all))
(def encode-fn (get plugin :encode))

## ── yaml/parse: simple mapping ──────────────────────────────────

(def result (parse-fn "name: hello\nversion: 1"))

(assert (= (get result :name) "hello") "yaml/parse name")

(assert (= (get result :version) 1) "yaml/parse version")

## ── yaml/parse: scalar types ────────────────────────────────────

(def types (parse-fn "i: 42\nf: 3.14\nb: true\nn: null\ns: hi"))

(assert (= (get types :i) 42) "yaml/parse int")

(assert (> (get types :f) 3.0) "yaml/parse float")

(assert (= (get types :b) true) "yaml/parse bool")

(assert (= (get types :n) nil) "yaml/parse null")

(assert (= (get types :s) "hi") "yaml/parse string")

## ── yaml/parse: sequence ────────────────────────────────────────

(def seq (parse-fn "- 1\n- 2\n- 3"))

(assert (= (length seq) 3) "yaml/parse sequence length")

(assert (= (get seq 0) 1) "yaml/parse sequence element")

## ── yaml/parse-all: multi-document ─────────────────────────────

(def docs (parse-all-fn "---\na: 1\n---\nb: 2"))

(assert (= (length docs) 2) "yaml/parse-all count")

(assert (= (get (get docs 0) :a) 1) "yaml/parse-all first doc")

(assert (= (get (get docs 1) :b) 2) "yaml/parse-all second doc")

## ── yaml/encode ─────────────────────────────────────────────────

(def encoded (encode-fn {:name "test" :count 5}))

(assert (string? encoded) "yaml/encode returns string")

## ── yaml roundtrip ──────────────────────────────────────────────

(def original {:x 1 :y "two" :z true})
(def rt (parse-fn (encode-fn original)))

(assert (= (get rt :x) 1) "yaml roundtrip x")

(assert (= (get rt :y) "two") "yaml roundtrip y")

(assert (= (get rt :z) true) "yaml roundtrip z")

## ── yaml nil roundtrip ──────────────────────────────────────────

(def nil-struct {:x nil})
(def nil-rt (parse-fn (encode-fn nil-struct)))

(assert (= (get nil-rt :x) nil) "yaml nil roundtrip")

## ── error: parse invalid YAML ───────────────────────────────────

(let [[ok? err] (protect ((fn () (parse-fn ":\n  - [invalid"))))]
  (assert (not ok?) "yaml/parse invalid")
  (assert (= (get err :error) :yaml-error) "yaml/parse invalid"))

## ── error: wrong type to parse ──────────────────────────────────

(let [[ok? _] (protect ((fn () (parse-fn 42))))]
  (assert (not ok?) "yaml/parse wrong type"))
