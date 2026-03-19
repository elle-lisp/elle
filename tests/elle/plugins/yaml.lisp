(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

## YAML plugin integration tests

## Try to load the yaml plugin. If it fails, exit cleanly.
(def [ok? plugin] (protect (import-file "target/release/libelle_yaml.so")))
(when (not ok?)
  (display "SKIP: yaml plugin not built\n")
  (exit 0))

## Extract plugin functions from the returned struct
(def parse-fn     (get plugin :parse))
(def parse-all-fn (get plugin :parse-all))
(def encode-fn    (get plugin :encode))

## ── yaml/parse: simple mapping ──────────────────────────────────

(def result (parse-fn "name: hello\nversion: 1"))

(assert-eq
  (get result :name)
  "hello"
  "yaml/parse name")

(assert-eq
  (get result :version)
  1
  "yaml/parse version")

## ── yaml/parse: scalar types ────────────────────────────────────

(def types (parse-fn "i: 42\nf: 3.14\nb: true\nn: null\ns: hi"))

(assert-eq
  (get types :i)
  42
  "yaml/parse int")

(assert-true
  (> (get types :f) 3.0)
  "yaml/parse float")

(assert-eq
  (get types :b)
  true
  "yaml/parse bool")

(assert-eq
  (get types :n)
  nil
  "yaml/parse null")

(assert-eq
  (get types :s)
  "hi"
  "yaml/parse string")

## ── yaml/parse: sequence ────────────────────────────────────────

(def seq (parse-fn "- 1\n- 2\n- 3"))

(assert-eq
  (length seq)
  3
  "yaml/parse sequence length")

(assert-eq
  (get seq 0)
  1
  "yaml/parse sequence element")

## ── yaml/parse-all: multi-document ─────────────────────────────

(def docs (parse-all-fn "---\na: 1\n---\nb: 2"))

(assert-eq
  (length docs)
  2
  "yaml/parse-all count")

(assert-eq
  (get (get docs 0) :a)
  1
  "yaml/parse-all first doc")

(assert-eq
  (get (get docs 1) :b)
  2
  "yaml/parse-all second doc")

## ── yaml/encode ─────────────────────────────────────────────────

(def encoded (encode-fn {:name "test" :count 5}))

(assert-true
  (string? encoded)
  "yaml/encode returns string")

## ── yaml roundtrip ──────────────────────────────────────────────

(def original {:x 1 :y "two" :z true})
(def rt (parse-fn (encode-fn original)))

(assert-eq
  (get rt :x)
  1
  "yaml roundtrip x")

(assert-eq
  (get rt :y)
  "two"
  "yaml roundtrip y")

(assert-eq
  (get rt :z)
  true
  "yaml roundtrip z")

## ── yaml nil roundtrip ──────────────────────────────────────────

(def nil-struct {:x nil})
(def nil-rt (parse-fn (encode-fn nil-struct)))

(assert-eq
  (get nil-rt :x)
  nil
  "yaml nil roundtrip")

## ── error: parse invalid YAML ───────────────────────────────────

(assert-err-kind
  (fn () (parse-fn ":\n  - [invalid"))
  :yaml-error
  "yaml/parse invalid")

## ── error: wrong type to parse ──────────────────────────────────

(assert-err
  (fn () (parse-fn 42))
  "yaml/parse wrong type")
