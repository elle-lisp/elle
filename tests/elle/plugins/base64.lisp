(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

## Base64 plugin integration tests
## Tests the base64 plugin (.so loaded via import-file)

## Try to load the base64 plugin. If it fails, exit cleanly.
(def [ok? plugin] (protect (import-file "target/release/libelle_base64.so")))
(when (not ok?)
  (display "SKIP: base64 plugin not built\n")
  (exit 0))

## Extract plugin functions from the returned struct
(def encode-fn     (get plugin :encode))
(def decode-fn     (get plugin :decode))
(def encode-url-fn (get plugin :encode-url))
(def decode-url-fn (get plugin :decode-url))

## ── base64/encode ───────────────────────────────────────────────

(assert-string-eq
  (encode-fn "hello")
  "aGVsbG8="
  "encode hello")

(assert-string-eq
  (encode-fn "")
  ""
  "encode empty string")

(assert-string-eq
  (encode-fn "Man")
  "TWFu"
  "encode Man (3 bytes, no padding)")

## ── base64/decode ───────────────────────────────────────────────

(assert-eq
  (decode-fn "aGVsbG8=")
  (bytes "hello")
  "decode hello")

(assert-eq
  (decode-fn "")
  (bytes "")
  "decode empty string")

## ── base64/encode-url ───────────────────────────────────────────

(assert-string-eq
  (encode-url-fn "hello")
  "aGVsbG8"
  "encode-url hello (no padding)")

(assert-string-eq
  (encode-url-fn "")
  ""
  "encode-url empty string")

## ── base64/decode-url ───────────────────────────────────────────

(assert-eq
  (decode-url-fn "aGVsbG8")
  (bytes "hello")
  "decode-url hello")

## ── @string input ───────────────────────────────────────────────

(assert-string-eq
  (encode-fn @"hello")
  "aGVsbG8="
  "encode @string input")

(assert-string-eq
  (encode-url-fn @"hello")
  "aGVsbG8"
  "encode-url @string input")

## ── bytes input to encode ───────────────────────────────────────

(assert-string-eq
  (encode-fn (bytes "hello"))
  "aGVsbG8="
  "encode bytes input")

(assert-string-eq
  (encode-url-fn (bytes "hello"))
  "aGVsbG8"
  "encode-url bytes input")

## ── invalid base64 → error ──────────────────────────────────────

(assert-err-kind
  (fn () (decode-fn "not!valid!base64!!!"))
  :base64-error
  "decode invalid base64")

(assert-err-kind
  (fn () (decode-url-fn "not!valid!base64!!!"))
  :base64-error
  "decode-url invalid base64")

## ── wrong type → error ──────────────────────────────────────────

(assert-err
  (fn () (encode-fn 42))
  "encode wrong type")

(assert-err
  (fn () (decode-fn 42))
  "decode wrong type")

(assert-err
  (fn () (encode-url-fn 42))
  "encode-url wrong type")

(assert-err
  (fn () (decode-url-fn 42))
  "decode-url wrong type")
