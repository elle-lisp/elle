(elle/epoch 9)
## tests/elle/grpc.lisp — gRPC client tests
##
## Part 1: framing (encode/decode) — no network needed.
## Part 2: integration — full gRPC-over-h2 against http2:serve with trailers.


## ── Dependencies ──────────────────────────────────────────────────

(def http2 ((import "std/http2")))

## Fake protobuf: encode returns raw bytes, decode returns a struct
(def fake-pb {:encode (fn [s t d] (bytes 1 2 3))
              :decode (fn [s t b] {:decoded true :len (length b)})})
(def grpc ((import "std/grpc") :http2 http2 :protobuf fake-pb))


## ── Helpers ───────────────────────────────────────────────────────

(defn listen-ephemeral []
  (let* [listener (tcp/listen "127.0.0.1" 0)
         lpath (port/path listener)
         lport (parse-int (slice lpath (+ 1 (string/find lpath ":"))))]
    [listener lport]))

(defn with-server [handler test-fn]
  "Start an h2-serve listener, run test-fn with session, clean up."
  (let* [[listener lport] (listen-ephemeral)
         sf (ev/spawn
           (fn []
             (let [[ok? _] (protect (http2:serve listener handler))]
               nil)))
         url (concat "http://127.0.0.1:" (string lport))
         session (http2:connect url)]
    (defer (begin (protect (http2:close session))
                  (protect (port/close listener))
                  (protect (ev/abort sf)))
      (test-fn session))))


## ── Part 1: Framing tests ────────────────────────────────────────

(defn test-encode-empty []
  (let [frame (grpc:encode (bytes))]
    (assert (= frame (bytes 0 0 0 0 0)) "encode empty: 5-byte header, zero length")))

(defn test-roundtrip-small []
  (let* [payload (bytes 10 20 30)
         frame (grpc:encode payload)
         decoded (grpc:decode frame)]
    (assert (= decoded payload) "roundtrip small payload")))

(defn test-roundtrip-multibyte-length []
  (let* [payload (apply bytes (map (fn [i] (% i 256)) (range 300)))
         frame (grpc:encode payload)
         decoded (grpc:decode frame)]
    (assert (= (length frame) (+ 5 300)) "multi-byte: frame length")
    (assert (= decoded payload) "multi-byte: roundtrip")))

(defn test-decode-nil []
  (assert (nil? (grpc:decode nil)) "decode nil"))

(defn test-decode-truncated []
  (assert (nil? (grpc:decode (bytes 0 0 0))) "decode truncated: 3 bytes")
  (assert (nil? (grpc:decode (bytes 0 0 0 0))) "decode truncated: 4 bytes"))

(defn test-decode-length-exceeds []
  (let [frame (bytes 0 0 0 0 10 1 2 3)]
    (assert (nil? (grpc:decode frame)) "decode: length exceeds data")))

(defn test-exports []
  (each key in [:connect :call :call-decode :call-stream :close :encode :decode]
    (assert (not (nil? (get grpc key)))
            (string "export key exists: " key))))


## ── Part 2: Integration tests (gRPC over h2 loopback) ───────────

(defn grpc-handler [req]
  "Route gRPC requests by path. Returns response with trailers."
  (let [path req:path
        body (or req:body (bytes))]
    (cond
      ## Echo the body back as gRPC response
      (= path "/test.Svc/Echo")
       (let [payload (grpc:decode body)]
         {:status 200
          :headers {:content-type "application/grpc"}
          :body (grpc:encode (or payload (bytes 7 8 9)))
          :trailers [["grpc-status" "0"]]})

      ## Return a fixed response
      (= path "/test.Svc/Fixed")
       {:status 200
        :headers {:content-type "application/grpc"}
        :body (grpc:encode (bytes 4 5 6))
        :trailers [["grpc-status" "0"]]}

      ## Return gRPC error in trailers
      (= path "/test.Svc/Error")
       {:status 200
        :headers {:content-type "application/grpc"}
        :body (bytes)
        :trailers [["grpc-status" "13"]
                   ["grpc-message" "internal error"]]}

      ## Trailers-only error (no body, status in trailers)
      (= path "/test.Svc/NotFound")
       {:status 200
        :headers {:content-type "application/grpc"}
        :trailers [["grpc-status" "5"]
                   ["grpc-message" "not found"]]}

      ## Empty success (no data)
      (= path "/test.Svc/Empty")
       {:status 200
        :headers {:content-type "application/grpc"}
        :trailers [["grpc-status" "0"]]}

      ## Large payload
      (= path "/test.Svc/Large")
       (let [big (apply bytes (map (fn [i] (% i 256)) (range 2000)))]
         {:status 200
          :headers {:content-type "application/grpc"}
          :body (grpc:encode big)
          :trailers [["grpc-status" "0"]]})

      ## Sequential — echo stream ID in response
      (string/starts-with? path "/test.Svc/Seq")
       {:status 200
        :headers {:content-type "application/grpc"}
        :body (grpc:encode (bytes 42))
        :trailers [["grpc-status" "0"]]}

      ## Server-streaming: multiple gRPC frames + trailers
      (= path "/test.Svc/Stream")
       {:status 200
        :headers {:content-type "application/grpc"}
        :body (concat (grpc:encode (bytes 10 11))
                      (grpc:encode (bytes 20 21))
                      (grpc:encode (bytes 30 31)))
        :trailers [["grpc-status" "0"]]}

      ## Default: not found
      true
       {:status 200
        :headers {:content-type "application/grpc"}
        :trailers [["grpc-status" "12"]
                   ["grpc-message" "unimplemented"]]})))

(defn test-unary-rpc []
  "Full unary gRPC call: connect → send → receive → decode."
  (with-server grpc-handler
    (fn [sess]
      (let [raw (grpc:call sess nil "/test.Svc/Fixed" "test.Req" {})]
        (assert (not (nil? raw)) "unary: got response bytes")
        (assert (= raw (bytes 4 5 6)) "unary: response payload matches")))))

(defn test-unary-decode []
  "Full unary gRPC call with decode: exercises grpc:call-decode."
  (with-server grpc-handler
    (fn [sess]
      (let [result (grpc:call-decode sess nil "/test.Svc/Fixed"
                     "test.Req" {} "test.Resp")]
        (assert (= result:decoded true) "decode: fake-pb decoded")
        (assert (= result:len 3) "decode: correct payload length")))))

(defn test-grpc-error-in-trailers []
  "Server returns grpc-status != 0 in trailers: client should raise."
  (with-server grpc-handler
    (fn [sess]
      (let [[ok? err] (protect
            (grpc:call sess nil "/test.Svc/Error" "test.Req" {}))]
        (assert (not ok?) "error: should raise")
        (assert (= err:error :grpc-error) "error: type is :grpc-error")
        (assert (= err:code 13) "error: code 13")
        (assert (= err:message "internal error") "error: message propagated")))))

(defn test-trailers-only-error []
  "Server sends trailers-only (no DATA), non-zero status."
  (with-server grpc-handler
    (fn [sess]
      (let [[ok? err] (protect
            (grpc:call sess nil "/test.Svc/NotFound" "test.Req" {}))]
        (assert (not ok?) "trailers-only: should raise")
        (assert (= err:code 5) "trailers-only: code 5")))))

(defn test-empty-response []
  "Server sends grpc-status 0 with no data — decoded result should be nil."
  (with-server grpc-handler
    (fn [sess]
      (let [raw (grpc:call sess nil "/test.Svc/Empty" "test.Req" {})]
        (assert (nil? raw) "empty response: nil")))))

(defn test-large-payload []
  "Unary RPC with payload > 1KB."
  (let [expected (apply bytes (map (fn [i] (% i 256)) (range 2000)))]
    (with-server grpc-handler
      (fn [sess]
        (let [raw (grpc:call sess nil "/test.Svc/Large" "test.Req" {})]
          (assert (= (length raw) 2000) "large: correct length")
          (assert (= raw expected) "large: payload matches"))))))

(defn test-sequential-rpcs []
  "Multiple sequential unary RPCs on the same session."
  (with-server grpc-handler
    (fn [sess]
      (each i in (range 5)
        (let [raw (grpc:call sess nil (concat "/test.Svc/Seq" (string i))
                    "test.Req" {})]
          (assert (not (nil? raw))
                  (concat "seq " (string i) ": got response")))))))


(defn test-server-streaming []
  "Server-streaming RPC: initial headers have no grpc-status, data arrives
   as multiple gRPC frames, grpc-status comes in trailers."
  (with-server grpc-handler
    (fn [sess]
      (let [reader (grpc:call-stream sess nil "/test.Svc/Stream"
                     "test.Req" {} "test.Resp")]
        (def msg1 (reader))
        (assert (not (nil? msg1)) "stream: got message 1")
        (def msg2 (reader))
        (assert (not (nil? msg2)) "stream: got message 2")
        (def msg3 (reader))
        (assert (not (nil? msg3)) "stream: got message 3")
        (def msg4 (reader))
        (assert (nil? msg4) "stream: nil at end")))))

## ── Run ──────────────────────────────────────────────────────────

(println "tests/elle/grpc.lisp:")

## Framing
(println "  framing:")
(test-encode-empty)
(println "    PASS: encode empty")
(test-roundtrip-small)
(println "    PASS: roundtrip small")
(test-roundtrip-multibyte-length)
(println "    PASS: roundtrip multi-byte length")
(test-decode-nil)
(println "    PASS: decode nil")
(test-decode-truncated)
(println "    PASS: decode truncated")
(test-decode-length-exceeds)
(println "    PASS: decode length exceeds")
(test-exports)
(println "    PASS: all exports present")

## Integration
(println "  integration:")
(test-unary-rpc)
(println "    PASS: unary RPC")
(test-unary-decode)
(println "    PASS: unary decode")
(test-grpc-error-in-trailers)
(println "    PASS: grpc error in trailers")
(test-trailers-only-error)
(println "    PASS: trailers-only error")
(test-empty-response)
(println "    PASS: empty response")
(test-large-payload)
(println "    PASS: large payload")
(test-sequential-rpcs)
(println "    PASS: sequential RPCs")
(test-server-streaming)
(println "    PASS: server-streaming (call-stream)")

(println "tests/elle/grpc.lisp: all tests passed")
