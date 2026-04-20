## tests/elle/grpc.lisp — gRPC framing tests
##
## Tests the pure framing functions (encode/decode) without needing a
## running gRPC server or the protobuf plugin.

(elle/epoch 8)

## ── Init with fake protobuf ──────────────────────────────────────

(def fake-pb {:encode (fn [s t d] (bytes 1 2 3))
              :decode (fn [s t b] {})})
(def http2 ((import "std/http2")))
(def grpc  ((import "std/grpc") :http2 http2 :protobuf fake-pb))

## ── Encode: empty payload ────────────────────────────────────────

(let [frame (grpc:encode (bytes))]
  (assert (= frame (bytes 0 0 0 0 0)) "encode empty: 5-byte header, zero length"))

## ── Encode/decode roundtrip: small payload ───────────────────────

(let* [payload (bytes 10 20 30)
       frame (grpc:encode payload)
       decoded (grpc:decode frame)]
  (assert (= decoded payload) "roundtrip small payload"))

## ── Encode/decode roundtrip: multi-byte length ───────────────────

(let* [payload (apply bytes (map (fn [i] (% i 256)) (range 300)))
       frame (grpc:encode payload)
       decoded (grpc:decode frame)]
  (assert (= (length frame) (+ 5 300)) "multi-byte: frame length")
  (assert (= decoded payload) "multi-byte: roundtrip"))

## ── Decode nil → nil ─────────────────────────────────────────────

(assert (nil? (grpc:decode nil)) "decode nil")

## ── Decode truncated frame → nil ─────────────────────────────────

(assert (nil? (grpc:decode (bytes 0 0 0))) "decode truncated: 3 bytes")
(assert (nil? (grpc:decode (bytes 0 0 0 0))) "decode truncated: 4 bytes")

## ── Decode length exceeding available bytes → nil ────────────────

(let [frame (bytes 0 0 0 0 10 1 2 3)]  # claims 10 bytes, only 3 available
  (assert (nil? (grpc:decode frame)) "decode: length exceeds data"))

## ── Verify all export keys exist ─────────────────────────────────

(each key in [:connect :call :call-decode :close :encode :decode]
  (assert (not (nil? (get grpc key)))
          (string "export key exists: " key)))

(println "tests/elle/grpc.lisp: all tests passed")
