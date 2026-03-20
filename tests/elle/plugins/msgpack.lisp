
## MessagePack plugin integration tests

(def [ok? plugin] (protect (import-file "target/release/libelle_msgpack.so")))
(when (not ok?)
  (display "SKIP: msgpack plugin not built\n")
  (exit 0))

(def encode-fn          (get plugin :encode))
(def decode-fn          (get plugin :decode))
(def valid-fn           (get plugin :valid?))
(def encode-tagged-fn   (get plugin :encode-tagged))
(def decode-tagged-fn   (get plugin :decode-tagged))

# ── Interop round-trip tests ────────────────────────────────────────

(assert (= (decode-fn (encode-fn nil)) nil) "nil round-trips")

(assert (= (decode-fn (encode-fn true)) true) "true round-trips")

(assert (= (decode-fn (encode-fn false)) false) "false round-trips")

(assert (= (decode-fn (encode-fn 0)) 0) "integer 0 round-trips")

(assert (= (decode-fn (encode-fn 1)) 1) "integer 1 round-trips")

(assert (= (decode-fn (encode-fn -1)) -1) "integer -1 round-trips")

(assert (= (decode-fn (encode-fn 127)) 127) "integer 127 (fixpos max) round-trips")

(assert (= (decode-fn (encode-fn -128)) -128) "integer -128 (fixneg min) round-trips")

(assert (= (decode-fn (encode-fn 256)) 256) "integer 256 (u16) round-trips")

(assert (= (decode-fn (encode-fn -32768)) -32768) "integer -32768 (i16 min) round-trips")

## Elle integers are 48-bit signed: max = 2^47-1 = 140737488355327
(assert (= (decode-fn (encode-fn 140737488355327)) 140737488355327) "integer 48-bit max round-trips")

(assert (= (decode-fn (encode-fn -140737488355328)) -140737488355328) "integer 48-bit min round-trips")

(assert (= (decode-fn (encode-fn 0.0)) 0.0) "float 0.0 round-trips")

(assert (= (decode-fn (encode-fn 1.5)) 1.5) "float 1.5 round-trips")

(assert (= (decode-fn (encode-fn -1.5)) -1.5) "float -1.5 round-trips")

(assert (= (decode-fn (encode-fn (float "inf"))) (float "inf")) "float +infinity round-trips")

(assert (= (decode-fn (encode-fn (float "-inf"))) (float "-inf")) "float -infinity round-trips")

## NaN: Elle = treats NaN as equal to itself (structural equality); assert-eq works
(assert (= (decode-fn (encode-fn (float "nan"))) (float "nan")) "NaN round-trips as NaN")

(assert (= (decode-fn (encode-fn "")) "") "empty string round-trips")

(assert (= (decode-fn (encode-fn "hello")) "hello") "short string round-trips")

(assert (= (decode-fn (encode-fn "this is a string longer than 31 characters for str8")) "this is a string longer than 31 characters for str8") "long string (>31 chars, uses str8 format) round-trips")

(assert (= (decode-fn (encode-fn (bytes))) (bytes)) "empty bytes round-trips")

(assert (= (decode-fn (encode-fn (bytes 1 2 3))) (bytes 1 2 3)) "non-empty bytes round-trips")

(assert (= (decode-fn (encode-fn [])) []) "empty array round-trips")

(assert (= (decode-fn (encode-fn [[1 2] [3 4]])) [[1 2] [3 4]]) "nested array round-trips")

(assert (= (decode-fn (encode-fn {})) {}) "empty struct round-trips")

(assert (= (decode-fn (encode-fn {"a" 1 "b" 2})) {"a" 1 "b" 2}) "struct with string keys round-trips")

(assert (= (decode-fn (encode-fn {1 "one" 2 "two"})) {1 "one" 2 "two"}) "struct with integer keys round-trips")

(assert (= (decode-fn (encode-fn {"a" [1 {"b" [2 3]}]})) {"a" [1 {"b" [2 3]}]}) "deeply nested struct/array round-trips")

# ── Interop lossy-conversion tests ──────────────────────────────────

## keyword → string (interop loses keyword identity)
(assert (= (decode-fn (encode-fn :foo)) "foo") "keyword becomes string in interop mode")

## set → array (interop loses set type)
(assert (array? (decode-fn (encode-fn |1 2 3|))) "set becomes array in interop mode")

## list → array (interop loses list type)
(assert (= (decode-fn (encode-fn (list 1 2 3))) [1 2 3]) "list becomes array in interop mode")

## empty list → empty array
(assert (= (decode-fn (encode-fn ())) []) "empty list becomes empty array in interop mode")

## mutable @array encodes same as immutable array
(assert (= (encode-fn @[1 2]) (encode-fn [1 2])) "@array and array produce same bytes")

## struct keyword keys become string keys in interop mode
(assert (has-key? (decode-fn (encode-fn {:x 1})) "x") "keyword key :x becomes string key \"x\" in interop mode")

# ── Tagged round-trip tests ──────────────────────────────────────────

## keyword preserves identity through tagged round-trip
(assert (= (decode-tagged-fn (encode-tagged-fn :foo)) :foo) "keyword round-trips via tagged mode")

## set preserves identity through tagged round-trip
(assert (= (decode-tagged-fn (encode-tagged-fn |1 2 3|)) |1 2 3|) "set round-trips via tagged mode")

## list preserves identity through tagged round-trip
(assert (= (decode-tagged-fn (encode-tagged-fn (list 1 2))) (list 1 2)) "list round-trips via tagged mode")

## empty list preserves identity
(assert (= (decode-tagged-fn (encode-tagged-fn ())) ()) "empty list round-trips via tagged mode")

## struct with keyword keys: keyword keys preserved in tagged mode
(let ((v (decode-tagged-fn (encode-tagged-fn {:x 1 :y 2}))))
  (assert (has-key? v :x) "keyword key :x preserved in tagged round-trip")
  (assert (= (get v :x) 1) "value at :x preserved in tagged round-trip"))

## complex nested structure
(let ((orig {:items (list :a :b) :count 2}))
  (assert (= (decode-tagged-fn (encode-tagged-fn orig)) orig) "nested struct with keyword keys and list round-trips via tagged mode"))

## shared types work identically in both modes
(assert (= (decode-tagged-fn (encode-tagged-fn nil)) nil) "nil works in tagged mode")
(assert (= (decode-tagged-fn (encode-tagged-fn 42)) 42) "int works in tagged mode")
(assert (= (decode-tagged-fn (encode-tagged-fn "hello")) "hello") "string works in tagged mode")
(assert (= (decode-tagged-fn (encode-tagged-fn [1 2 3])) [1 2 3]) "array works in tagged mode")
(assert (= (decode-tagged-fn (encode-tagged-fn (bytes 1 2 3))) (bytes 1 2 3)) "bytes works in tagged mode")

# ── Cross-mode compatibility tests ──────────────────────────────────

## decode-tagged can decode interop-encoded bytes for shared types
(assert (= (decode-tagged-fn (encode-fn 42)) 42) "decode-tagged handles interop-encoded int")

(assert (= (decode-tagged-fn (encode-fn "hello")) "hello") "decode-tagged handles interop-encoded string")

(assert (= (decode-tagged-fn (encode-fn [1 2 3])) [1 2 3]) "decode-tagged handles interop-encoded array")

## decode (interop) on tagged-encoded bytes with ext → error
(let (([ok? _] (protect ((fn () (decode-fn (encode-tagged-fn :foo))))))) (assert (not ok?) "interop decode on tagged keyword bytes gives error"))

# ── valid? tests ─────────────────────────────────────────────────────

(assert (valid-fn (encode-fn 42)) "valid? true for valid interop bytes")

(assert (valid-fn (encode-tagged-fn :foo)) "valid? true for tagged bytes with ext (ext is structurally valid)")

(assert (not (valid-fn (bytes 0xc1))) "valid? false for reserved marker 0xc1")

(assert (not (valid-fn (bytes))) "valid? false for empty bytes")

(assert (not (valid-fn 42)) "valid? false for non-bytes input (integer)")

(assert (not (valid-fn (bytes 0xc0 0xc0))) "valid? false for two values (trailing bytes)")

(assert (not (valid-fn (bytes 0x92 0x01))) "valid? false for truncated fixarray(2) with only 1 element")

# ── Error tests ───────────────────────────────────────────────────────

(let (([ok? _] (protect ((fn () (encode-fn (fn () 42))))))) (assert (not ok?) "encoding a closure is an error"))

## Improper list: (cons 1 2) has non-list cdr
(let (([ok? _] (protect ((fn () (encode-fn (cons 1 2))))))) (assert (not ok?) "encoding an improper list is an error"))

(let (([ok? err] (protect ((fn () (decode-fn "not bytes")))))) (assert (not ok?) "decode with string input gives type-error") (assert (= (get err :error) :type-error) "decode with string input gives type-error"))

(let (([ok? _] (protect ((fn () (decode-fn (bytes 0xc1))))))) (assert (not ok?) "decode with reserved marker gives error"))

(let (([ok? _] (protect ((fn () (decode-fn (bytes))))))) (assert (not ok?) "decode with empty bytes gives error"))

(let (([ok? _] (protect ((fn () (decode-fn (bytes 0x92 0x01))))))) (assert (not ok?) "decode with truncated array gives error"))
