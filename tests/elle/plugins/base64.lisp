
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

(assert (= (encode-fn "hello") "aGVsbG8=") "encode hello")

(assert (= (encode-fn "") "") "encode empty string")

(assert (= (encode-fn "Man") "TWFu") "encode Man (3 bytes, no padding)")

## ── base64/decode ───────────────────────────────────────────────

(assert (= (decode-fn "aGVsbG8=") (bytes "hello")) "decode hello")

(assert (= (decode-fn "") (bytes "")) "decode empty string")

## ── base64/encode-url ───────────────────────────────────────────

(assert (= (encode-url-fn "hello") "aGVsbG8") "encode-url hello (no padding)")

(assert (= (encode-url-fn "") "") "encode-url empty string")

## ── base64/decode-url ───────────────────────────────────────────

(assert (= (decode-url-fn "aGVsbG8") (bytes "hello")) "decode-url hello")

## ── @string input ───────────────────────────────────────────────

(assert (= (encode-fn @"hello") "aGVsbG8=") "encode @string input")

(assert (= (encode-url-fn @"hello") "aGVsbG8") "encode-url @string input")

## ── bytes input to encode ───────────────────────────────────────

(assert (= (encode-fn (bytes "hello")) "aGVsbG8=") "encode bytes input")

(assert (= (encode-url-fn (bytes "hello")) "aGVsbG8") "encode-url bytes input")

## ── invalid base64 → error ──────────────────────────────────────

(let (([ok? err] (protect ((fn () (decode-fn "not!valid!base64!!!")))))) (assert (not ok?) "decode invalid base64") (assert (= (get err :error) :base64-error) "decode invalid base64"))

(let (([ok? err] (protect ((fn () (decode-url-fn "not!valid!base64!!!")))))) (assert (not ok?) "decode-url invalid base64") (assert (= (get err :error) :base64-error) "decode-url invalid base64"))

## ── wrong type → error ──────────────────────────────────────────

(let (([ok? _] (protect ((fn () (encode-fn 42)))))) (assert (not ok?) "encode wrong type"))

(let (([ok? _] (protect ((fn () (decode-fn 42)))))) (assert (not ok?) "decode wrong type"))

(let (([ok? _] (protect ((fn () (encode-url-fn 42)))))) (assert (not ok?) "encode-url wrong type"))

(let (([ok? _] (protect ((fn () (decode-url-fn 42)))))) (assert (not ok?) "decode-url wrong type"))
