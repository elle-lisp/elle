(elle/epoch 11)
## infra/h2cross.lisp — differential h2 echo-amplify crosstest
##
## Tests all 4 permutations: {elle,rust} × {client,server} to isolate
## flow control bugs in bidi streaming.
##
## Composable hypothesis flags:
##   grpc-framing  H2: gRPC 5-byte prefix instead of 4-byte length prefix
##   trailers      H3: END_STREAM on trailing HEADERS, not on last DATA
##   window-size   H4: server initial window size (e.g., 67108864 for 64MB)
##
## Usage (run from grace repo root):
##   elle infra/h2cross.lisp
##   elle infra/h2cross.lisp elle-elle
##   elle infra/h2cross.lisp elle-rust
##   elle infra/h2cross.lisp rust-elle
##   elle infra/h2cross.lisp rust-rust
##   elle infra/h2cross.lisp h134       # hypothesis tests (all 4 permutations)
##   elle infra/h2cross.lisp h2         # individual hypotheses
##   elle infra/h2cross.lisp h3
##   elle infra/h2cross.lisp h4

(def http2 ((import "std/http2")))

## ── Config ────────────────────────────────────────────────────────────────

(def DEFAULT-COUNT 650)
(def DEFAULT-REQUEST-SIZE 100)
(def DEFAULT-RESPONSE-SIZE 12000)
(def H2CROSS-BIN
  (concat (sys/env "HOME") "/git/grace/infra/h2cross/target/release/h2cross"))
(def ELLE-BIN (or (sys/env "ELLE_BIN") "elle/target/release/elle"))
(def TIMEOUT 60)
# seconds per test

## ── Plain message framing ───────────────────────────────────────────────
## 4-byte big-endian length prefix + payload

(defn encode-plain [payload]
  (let [len (length payload)]
    (concat (bytes (bit/and (bit/shr len 24) 0xff)
                   (bit/and (bit/shr len 16) 0xff)
                   (bit/and (bit/shr len 8) 0xff) (bit/and len 0xff)) payload)))

(defn decode-plain [buf]
  "Decode 4-byte length-prefixed messages. Returns [messages remaining-buf]."
  (let [@msgs @[]
        @pos 0]
    (while (>= (- (length buf) pos) 4)
      (let [len (+ (bit/shl (get buf pos) 24) (bit/shl (get buf (+ pos 1)) 16)
                   (bit/shl (get buf (+ pos 2)) 8) (get buf (+ pos 3)))]
        (if (< (- (length buf) pos) (+ 4 len))
          (break nil)
          (begin
            (push msgs (slice buf (+ pos 4) (+ pos 4 len)))
            (assign pos (+ pos 4 len))))))
    [msgs (slice buf pos)]))

## ── gRPC message framing ────────────────────────────────────────────────
## 1-byte compressed flag (0x00) + 4-byte BE length + payload

(defn encode-grpc [payload]
  (let [len (length payload)]
    (concat (bytes 0x00 (bit/and (bit/shr len 24) 0xff)
                   (bit/and (bit/shr len 16) 0xff)
                   (bit/and (bit/shr len 8) 0xff) (bit/and len 0xff)) payload)))

(defn decode-grpc [buf]
  "Decode gRPC 5-byte framed messages. Returns [messages remaining-buf]."
  (let [@msgs @[]
        @pos 0]
    (while (>= (- (length buf) pos) 5)
      (let [len (+ (bit/shl (get buf (+ pos 1)) 24)
                   (bit/shl (get buf (+ pos 2)) 16)
                   (bit/shl (get buf (+ pos 3)) 8) (get buf (+ pos 4)))]
        (if (< (- (length buf) pos) (+ 5 len))
          (break nil)
          (begin
            (push msgs (slice buf (+ pos 5) (+ pos 5 len)))
            (assign pos (+ pos 5 len))))))
    [msgs (slice buf pos)]))

## ── Dispatch helpers ────────────────────────────────────────────────────

(defn encode-msg [payload grpc-framing]
  (if grpc-framing (encode-grpc payload) (encode-plain payload)))

(defn decode-msgs [buf grpc-framing]
  (if grpc-framing (decode-grpc buf) (decode-plain buf)))

(defn prefix-size [grpc-framing]
  (if grpc-framing 5 4))

(defn make-payload [size]
  (let [@chunks @[]]
    (each _ in (range 0 (/ size 20))
      (push chunks (bytes 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19)))
    (let [result (apply concat chunks)
          got (length result)]
      (if (>= got size)
        (slice result 0 size)
        (concat result (bytes (mod 0 (max 1 (- size got)))))))))

(defn ts []
  (string (clock/monotonic)))

(defn flags-str [opts]
  (let [@parts @[]]
    (when opts:grpc-framing (push parts "grpc-framing"))
    (when opts:trailers (push parts "trailers"))
    (when opts:window-size
      (push parts (concat "window=" (string opts:window-size))))
    (when opts:close-after
      (push parts (concat "close@" (string opts:close-after))))
    (if (empty? parts) "plain" (string/join parts "+"))))

## ── Elle server (streaming) ──────────────────────────────────────────────
## Uses http2:serve-streaming so each response is a separate DATA frame (H1).
## Supports all hypothesis flags: grpc-framing, trailers, window-size.

(defn elle-server [port response-size opts]
  "Start an Elle h2 streaming echo-amplify server. Returns [listener port fiber]."
  (let [listener (tcp/listen "127.0.0.1" port)
        lpath (port/path listener)
        lport (parse-int (slice lpath (+ 1 (string/find lpath ":"))))
        resp-payload (make-payload response-size)
        gf opts:grpc-framing
        use-trailers opts:trailers
        handler (fn [req ctrl]  # Read all client messages from body
                  (def @body-buf (bytes))
                  (forever
                    (let [data (ctrl:recv)]
                      (when (nil? data) (break nil))
                      (assign body-buf (concat body-buf data))))
                  (let [[msgs _] (decode-msgs body-buf gf)
                        msg-count (length msgs)]
                    (eprintln "[" (ts) "] elle-server[" (flags-str opts)
                              "]: received " msg-count " messages ("
                              (length body-buf) " bytes), sending " msg-count
                              " × " response-size " byte responses")  # Send response headers
                    (if gf
                      (ctrl:send-headers 200
                      :headers {:content-type "application/grpc"})
                      (ctrl:send-headers 200))  # H1: Send each response as a separate DATA frame
                    (each i in (range 0 msg-count)
                      (ctrl:send-data (encode-msg resp-payload gf))
                      (when (= 0 (mod (+ i 1) 64))
                        (eprintln "[" (ts) "] elle-server: sent response "
                                  (+ i 1) "/" msg-count)))  # Close stream: trailers or plain end-stream
                    (if use-trailers
                      (ctrl:send-trailers [["grpc-status" "0"]
                      ["grpc-message" "OK"]])
                      (ctrl:end-stream))
                    (eprintln "[" (ts) "] elle-server: done sending " msg-count
                              " responses")))
        fiber (ev/spawn (fn []
                          (let [[ok? err] (protect (http2:serve-streaming listener
                                handler))]
                            (unless ok? (eprintln "elle-server error: " err)))))]
    (eprintln "[" (ts) "] elle-server[" (flags-str opts) "]: listening on port "
              lport)
    [listener lport fiber]))

## ── Elle client ─────────────────────────────────────────────────────────

(defn elle-client [target-port count request-size response-size opts]
  "Run Elle h2 client. Returns result map."
  (let [url (concat "http://127.0.0.1:" (string target-port))
        session (http2:connect url)
        payload (make-payload request-size)
        gf opts:grpc-framing
        expect-trailers opts:trailers
        start (clock/monotonic)]
    (defer
      (protect (http2:close session))

      (def extra-headers
        (if gf
          [["x-h2cross" "true"] ["content-type" "application/grpc"]]
          [["x-h2cross" "true"]]))

      (def [sid s]
        (http2:open-stream session "POST" "/echo-amplify" :headers extra-headers))

      # Send all messages
      (each i in (range 0 count)
        (let [msg (encode-msg payload gf)]
          (http2:stream-send session sid msg))
        (when (= 0 (mod (+ i 1) 64))
          (eprintln "[" (ts) "] elle-client[" (flags-str opts) "]: sent "
                    (+ i 1) "/" count)))

      # Half-close
      (http2:stream-end session sid)
      (eprintln "[" (ts) "] elle-client[" (flags-str opts) "]: sent " count
                " messages, half-closed, reading responses")

      # Read all responses from data-queue
      (def @buf (bytes))
      (def @msg-count 0)
      (def @total-bytes 0)
      (def @done false)
      (def @frame-count 0)
      (def @got-trailers false)

      (while (not done)
        (let [msg (s:data-queue:take)]
          (when (nil? msg) (assign done true))
          (unless (nil? msg)
            (match msg:type
              :headers
                (begin  # Initial response headers or trailers
                  # Trailers arrive as second :headers with end-stream
                  (when msg:end-stream
                    (assign got-trailers true)
                    (assign done true))
                  (eprintln "[" (ts) "] elle-client: recv HEADERS"
                            " end-stream=" msg:end-stream))
              :data
                (begin
                  (assign frame-count (+ frame-count 1))
                  (assign total-bytes (+ total-bytes (length msg:data)))
                  (assign buf (concat buf msg:data))
                  (when (= 0 (mod frame-count 64))
                    (eprintln "[" (ts) "] elle-client: recv DATA frame #"
                              frame-count ", " total-bytes " bytes total"))  # In trailers mode, END_STREAM should NOT be on DATA
                  # but handle it defensively
                  (when msg:end-stream
                    (when expect-trailers
                      (eprintln "[" (ts) "] WARNING: END_STREAM on DATA"
                                " (unexpected with trailers)"))
                    (assign done true)))
              _ (assign done true))))

        # Decode messages from buffer
        (let [[msgs remaining] (decode-msgs buf gf)]
          (assign msg-count (+ msg-count (length msgs)))
          (assign buf remaining)))

      (let [elapsed (- (clock/monotonic) start)
            expected-bytes (* count (+ (prefix-size gf) response-size))
            pass (and (= msg-count count)
                      (or (not expect-trailers) got-trailers))]
        (eprintln "[" (ts) "] elle-client[" (flags-str opts) "]: done — "
                  msg-count " messages, " total-bytes " bytes, " frame-count
                  " DATA frames, trailers=" got-trailers)
        {:messages-sent count
         :messages-received msg-count
         :bytes-received total-bytes
         :expected-bytes expected-bytes
         :elapsed-ms (round (* elapsed 1000))
         :pass pass}))))

## ── Rust subprocess helpers ─────────────────────────────────────────────

(defn rust-server-args [response-size opts]
  "Build CLI args for h2cross server."
  (let [@args @["server" "--addr" "127.0.0.1:0" "--response-size"
                (string response-size)]]
    (when opts:grpc-framing (push args "--grpc-framing"))
    (when opts:trailers (push args "--trailers"))
    (when opts:window-size
      (push args "--window-size")
      (push args (string opts:window-size)))
    args))

(defn rust-client-args [target-port count request-size response-size opts]
  "Build CLI args for h2cross client."
  (let [@args @["client" "--target" (concat "127.0.0.1:" (string target-port))
                "--count" (string count) "--request-size" (string request-size)
                "--response-size" (string response-size)]]
    (when opts:grpc-framing (push args "--grpc-framing"))
    (when opts:trailers (push args "--trailers"))
    (when opts:close-after
      (push args "--close-after")
      (push args (string opts:close-after)))
    args))

(defn start-rust-server [response-size opts]
  "Start h2cross server as subprocess. Returns [process port]."
  (let [args (rust-server-args response-size opts)
        proc (subprocess/exec H2CROSS-BIN args)]
    (ev/spawn (fn []
                (while true
                  (let [chunk (port/read proc:stderr 4096)]
                    (when (nil? chunk) (break nil))
                    (eprint (string chunk))))))
    (let [line (port/read-line proc:stdout)]
      (when (nil? line) (error "rust server exited before printing port"))
      (let [parts (string/split line " ")
            port (parse-int (get parts 1))]
        (eprintln "[" (ts) "] rust server[" (flags-str opts)
                  "] started on port " port)
        [proc port]))))

(defn start-rust-client [target-port count request-size response-size opts]
  "Run h2cross client as subprocess. Returns result map."
  (let [args (rust-client-args target-port count request-size response-size opts)
        result (subprocess/system H2CROSS-BIN args)]
    (when (> (length result:stderr) 0) (eprint result:stderr))
    (let [lines (string/split result:stdout "\n")
          @parsed @{:pass (= result:exit 0)}]
      (each line in lines
        (let [trimmed (string/trim line)]
          (when (string/starts-with? trimmed "messages_received=")
            (put parsed :messages-received (parse-int (slice trimmed 18))))
          (when (string/starts-with? trimmed "messages_sent=")
            (put parsed :messages-sent (parse-int (slice trimmed 14))))
          (when (string/starts-with? trimmed "bytes_received=")
            (put parsed :bytes-received (parse-int (slice trimmed 15))))
          (when (string/starts-with? trimmed "elapsed_ms=")
            (put parsed :elapsed-ms (parse-int (slice trimmed 11))))
          (when (string/starts-with? trimmed "got_trailers=")
            (put parsed :got-trailers (= (slice trimmed 13) "true")))
          (when (string/starts-with? trimmed "status=")
            (put parsed :pass (= (slice trimmed 7) "PASS")))))
      parsed)))

## ── Test runner ─────────────────────────────────────────────────────────

(defn
  run-permutation
  [name server-type client-type count request-size response-size opts]
  "Run one permutation. Returns result map."
  (println "\n── " name " (" count " msgs, " request-size "→"
           response-size " bytes) ──")

  (let [@server-proc nil
        @server-port 0
        @server-listener nil
        @server-fiber nil]
    (if (= server-type :rust)
      (let [[proc port] (start-rust-server response-size opts)]
        (assign server-proc proc)
        (assign server-port port))
      (let [[listener port fiber] (elle-server 0 response-size opts)]
        (assign server-listener listener)
        (assign server-port port)
        (assign server-fiber fiber)))

    # Small delay for server startup
    (ev/sleep 0.1)

    (let [result (defer
                   (begin
                     (when server-proc
                       (protect (subprocess/kill server-proc))
                       (protect (subprocess/wait server-proc)))
                     (when server-listener
                       (protect (port/close server-listener)))
                     (when server-fiber (protect (ev/abort server-fiber))))

                   # Run client
                   (if (= client-type :rust)
                     (start-rust-client server-port count request-size
                                        response-size opts)
                     (elle-client server-port count request-size response-size
                                  opts)))]
      (println "  messages_sent=" (or result:messages-sent "?"))
      (println "  messages_received=" (or result:messages-received "?"))
      (println "  bytes_received=" (or result:bytes-received "?"))
      (println "  elapsed_ms=" (or result:elapsed-ms "?"))
      (println "  status=" (if result:pass "PASS" "FAIL"))
      result)))

(defn
  run-sweep
  [server-type client-type name counts opts &named request-size response-size]
  "Run a sweep of counts for one server/client permutation."
  (let [req-sz (or request-size DEFAULT-REQUEST-SIZE)
        resp-sz (or response-size DEFAULT-RESPONSE-SIZE)
        @results @[]]
    (each count in counts
      (let [[ok? result] (protect (ev/timeout TIMEOUT
                                  (fn []
                                    (run-permutation (concat name " count="
                                    (string count)) server-type client-type
                                    count req-sz resp-sz opts))))]
        (if ok?
          (if (nil? result)
            (begin
              (println "  status=TIMEOUT")
              (push results {:count count :pass false :timeout true}))
            (push results
                  @{:count count
                    :pass result:pass
                    :messages-received result:messages-received
                    :messages-sent result:messages-sent
                    :bytes-received result:bytes-received
                    :elapsed-ms result:elapsed-ms}))
          (begin
            (println "  status=ERROR: " result)
            (push results {:count count :pass false :error result})))))
    results))

## ── Differential test: all 4 permutations ──────────────────────────────

(defn run-4way [label opts &named counts request-size response-size]
  "Run all 4 permutations with the given opts. Returns summary."
  (let [cts (or counts [384 385 650])
        fs (flags-str opts)
        ee (run-sweep :elle :elle (concat "elle→elle[" fs "]") cts opts
                      :request-size request-size :response-size response-size)
        rr (run-sweep :rust :rust (concat "rust→rust[" fs "]") cts opts
                      :request-size request-size :response-size response-size)
        re (run-sweep :rust :elle (concat "elle→rust[" fs "]") cts opts
                      :request-size request-size :response-size response-size)
        er (run-sweep :elle :rust (concat "rust→elle[" fs "]") cts opts
                      :request-size request-size :response-size response-size)]
    [[(concat "elle→elle[" fs "]") ee] [(concat "rust→rust[" fs "]") rr]
     [(concat "elle→rust[" fs "]") re] [(concat "rust→elle[" fs "]") er]]))

(defn print-summary [label all-results]
  (println "\n══ " label " ══")
  (each [name results] in all-results
    (println name ":")
    (each r in results
      (let [status (cond
                     r:timeout "TIMEOUT"
                     r:error (concat "ERROR: " (string r:error))
                     r:pass "PASS"
                     true
                       (concat "FAIL (got "
                               (string (or r:messages-received "?")) ")"))]
        (println "  count=" r:count " → " status)))))

## ── Main ────────────────────────────────────────────────────────────────

(def PLAIN-OPTS {:grpc-framing false :trailers false :window-size nil})

(defn run-all []
  (println "h2cross: differential HTTP/2 echo-amplify test (plain mode)")
  (println "  request_size=" DEFAULT-REQUEST-SIZE)
  (println "  response_size=" DEFAULT-RESPONSE-SIZE)
  (println "  timeout=" TIMEOUT "s per test")
  (println "  h2cross_bin=" H2CROSS-BIN)
  (let [results (run-4way "plain" PLAIN-OPTS)]
    (print-summary "Plain Summary" results)))

(defn run-hypothesis [label opts &named counts request-size response-size]
  "Run baseline (plain) + hypothesis, all 4 permutations each."
  (let [req-sz (or request-size DEFAULT-REQUEST-SIZE)
        resp-sz (or response-size DEFAULT-RESPONSE-SIZE)
        cts (or counts [384 385 650])]
    (println "h2cross: hypothesis test — " label)
    (println "  flags: " (flags-str opts))
    (println "  request_size=" req-sz)
    (println "  response_size=" resp-sz)
    (println "  counts=" cts)
    (println "  timeout=" TIMEOUT "s per test")
    (let [baseline (run-4way "baseline" PLAIN-OPTS :counts cts
                             :request-size req-sz :response-size resp-sz)
          test (run-4way label opts :counts cts :request-size req-sz
                         :response-size resp-sz)]
      (print-summary "Baseline (plain)" baseline)
      (print-summary (concat "Hypothesis: " label) test))))

## ── H8: concurrent streams ──────────────────────────────────────────────
## Open two streams: one for echo-amplify (the "real" stream), one idle
## that just holds connection window. Tests whether connection-level flow
## control starvation causes stalls.

(defn elle-client-h8 [target-port count request-size response-size opts]
  "Elle client with concurrent idle stream (H8)."
  (let [url (concat "http://127.0.0.1:" (string target-port))
        session (http2:connect url)
        payload (make-payload request-size)
        gf opts:grpc-framing
        expect-trailers opts:trailers
        start (clock/monotonic)]
    (defer
      (protect (http2:close session))

      # Open an idle stream that holds open but sends nothing useful
      (def [idle-sid idle-s]
        (http2:open-stream session "POST" "/idle"
                           :headers [["x-h2cross" "idle"]]))

      (def extra-headers
        (if gf
          [["x-h2cross" "true"] ["content-type" "application/grpc"]]
          [["x-h2cross" "true"]]))
      (def [sid s]
        (http2:open-stream session "POST" "/echo-amplify" :headers extra-headers))

      # Send all messages on the real stream
      (each i in (range 0 count)
        (let [msg (encode-msg payload gf)]
          (http2:stream-send session sid msg))
        (when (= 0 (mod (+ i 1) 64))
          (eprintln "[" (ts) "] elle-client[H8]: sent " (+ i 1) "/" count)))

      # Half-close the real stream
      (http2:stream-end session sid)
      (eprintln "[" (ts)
                "] elle-client[H8]: half-closed real stream, idle stream open")

      # Read all responses from real stream
      (def @buf (bytes))
      (def @msg-count 0)
      (def @total-bytes 0)
      (def @done false)
      (def @frame-count 0)
      (def @got-trailers false)

      (while (not done)
        (let [msg (s:data-queue:take)]
          (when (nil? msg) (assign done true))
          (unless (nil? msg)
            (match msg:type
              :headers
                (begin
                  (when msg:end-stream
                    (assign got-trailers true)
                    (assign done true)))
              :data
                (begin
                  (assign frame-count (+ frame-count 1))
                  (assign total-bytes (+ total-bytes (length msg:data)))
                  (assign buf (concat buf msg:data))
                  (when (= 0 (mod frame-count 64))
                    (eprintln "[" (ts) "] elle-client[H8]: recv DATA frame #"
                              frame-count ", " total-bytes " bytes total"))
                  (when msg:end-stream (assign done true)))
              _ (assign done true))))

        (let [[msgs remaining] (decode-msgs buf gf)]
          (assign msg-count (+ msg-count (length msgs)))
          (assign buf remaining)))

      # Close idle stream
      (http2:stream-end session idle-sid)

      (let [elapsed (- (clock/monotonic) start)
            expected-bytes (* count (+ (prefix-size gf) response-size))
            pass (and (= msg-count count)
                      (or (not expect-trailers) got-trailers))]
        (eprintln "[" (ts) "] elle-client[H8]: done — " msg-count " messages")
        {:messages-sent count
         :messages-received msg-count
         :bytes-received total-bytes
         :expected-bytes expected-bytes
         :elapsed-ms (round (* elapsed 1000))
         :pass pass}))))

(defn run-h8 []
  (println "h2cross: H8 — concurrent streams sharing connection window")
  (println "  Two streams open: echo-amplify + idle")
  (let [counts [384 385 650]
        opts PLAIN-OPTS
        @results @[]]
    (each [st ct name] in [[:elle :elle "elle→elle[H8]"]
                           [:rust :elle "elle→rust[H8]"]]
      (each count in counts
        (let [[ok? result] (protect (ev/timeout TIMEOUT
                                    (fn []
                                      (println "\n── " name " count=" count
                                      " ──")
                                      (let [@server-proc nil
                                        @server-port 0
                                        @server-listener nil
                                        @server-fiber nil]
                                        (if (= st :rust)
                                          (let [[proc port] (start-rust-server DEFAULT-RESPONSE-SIZE
                                            opts)]
                                            (assign server-proc proc)
                                            (assign server-port port))
                                          (let [[listener port fiber] (elle-server 0
                                            DEFAULT-RESPONSE-SIZE opts)]
                                            (assign server-listener listener)
                                            (assign server-port port)
                                            (assign server-fiber fiber)))
                                        (ev/sleep 0.1)
                                        (defer
                                          (begin
                                            (when server-proc
                                              (protect (subprocess/kill server-proc))
                                              (protect (subprocess/wait server-proc)))
                                            (when server-listener
                                              (protect (port/close server-listener)))
                                            (when server-fiber
                                              (protect (ev/abort server-fiber))))
                                          (let [result (elle-client-h8 server-port
                                            count DEFAULT-REQUEST-SIZE
                                            DEFAULT-RESPONSE-SIZE opts)]
                                            (println "  status="
                                            (if result:pass "PASS" "FAIL"))
                                            result))))))]
          (if ok?
            (if (nil? result)
              (begin
                (println "  status=TIMEOUT")
                (push results
                      {:name name :count count :pass false :timeout true}))
              (push results @{:name name :count count :pass result:pass}))
            (begin
              (println "  status=ERROR: " result)
              (push results {:name name :count count :pass false :error result}))))))
    (println "\n══ H8 Summary ══")
    (each r in results
      (let [status (cond
                     r:timeout "TIMEOUT"
                     r:error "ERROR"
                     r:pass "PASS"
                     true "FAIL")]
        (println "  " r:name " count=" r:count " → " status)))))

## ── H9: PING keepalive ─────────────────────────────────────────────────
## The h2 crate doesn't expose keepalive PING directly in server::Builder,
## but tonic/hyper does. For this test, we check if PINGs sent by the
## server during load cause issues. The h2-debug output will show any
## GOAWAY frames received. Since we can't configure h2 crate pings easily,
## we run the standard test and look for GOAWAY in the output.

(defn run-h9 []
  (println "h2cross: H9 — PING/GOAWAY observation")
  (println "  The h2 crate doesn't support server-side keepalive PINGs directly.")
  (println "  Running standard test with close observation of GOAWAY in stderr.")
  (println "  If tonic's keepalive causes issues, we'd see GOAWAY frames.")
  (let [results (run-4way "H9: PING observation" PLAIN-OPTS)]
    (print-summary "H9: PING observation" results)
    (println "\n  NOTE: Check stderr for GOAWAY frames. None means H9 is not the cause.")))

## ── H10: connection recycling race ─────────────────────────────────────
## Simulate closing the session mid-stream. If grace.lisp's RECYCLE-INTERVAL
## triggers during a bidi stream, the session is closed mid-read.
## close-after in opts tells the client to stop reading after N messages
## and drop/close the connection.

(defn elle-client-h10 [target-port count request-size response-size opts]
  "Elle client that closes after receiving close-after messages (H10)."
  (let [url (concat "http://127.0.0.1:" (string target-port))
        session (http2:connect url)
        payload (make-payload request-size)
        gf opts:grpc-framing
        ca opts:close-after
        start (clock/monotonic)]
    (def [sid s]
      (http2:open-stream session "POST" "/echo-amplify"
                         :headers [["x-h2cross" "true"]]))

    # Send all messages
    (each i in (range 0 count)
      (let [msg (encode-msg payload gf)]
        (http2:stream-send session sid msg)))

    (http2:stream-end session sid)
    (eprintln "[" (ts) "] elle-client[H10]: sent " count
              " messages, half-closed, close-after=" ca)

    # Read responses, stop after close-after messages
    (def @buf (bytes))
    (def @msg-count 0)
    (def @done false)
    (def @frame-count 0)

    (while (not done)
      (let [msg (s:data-queue:take)]
        (when (nil? msg) (assign done true))
        (unless (nil? msg)
          (match msg:type
            :headers nil
            :data
              (begin
                (assign frame-count (+ frame-count 1))
                (assign buf (concat buf msg:data))
                (when msg:end-stream (assign done true)))
            _ (assign done true))))

      (let [[msgs remaining] (decode-msgs buf gf)]
        (assign msg-count (+ msg-count (length msgs)))
        (assign buf remaining))

      # Simulate recycling: close after N messages
      (when (and ca (>= msg-count ca) (not done))
        (eprintln "[" (ts) "] elle-client[H10]: got " msg-count
                  " messages, simulating recycle (close)")
        (assign done true)))

    (let [elapsed (- (clock/monotonic) start)
          pass (>= msg-count ca)]
      (eprintln "[" (ts) "] elle-client[H10]: got " msg-count
                " messages before close")  # Graceful close — data-queue:close unblocks the reader fiber
      (protect (http2:close session))
      {:messages-sent count
       :messages-received msg-count
       :bytes-received 0
       :expected-bytes 0
       :elapsed-ms (round (* elapsed 1000))
       :pass pass})))

(defn run-h10 []
  (println "h2cross: H10 — connection recycling race (all 4 permutations)")
  (println "  close-after = count/2 (close session after receiving half the responses)")
  (let [counts [384 650]
        half-opts (fn [count]
                    {:grpc-framing false
                     :trailers false
                     :window-size nil
                     :close-after (/ count 2)})
        @all-results @[]]
    (each count in counts
      (let [opts (half-opts count)
            fs (concat "H10:close@" (string (/ count 2)))]
        (each [st ct name] in [[:elle :elle "elle→elle"]
                               [:rust :rust "rust→rust"]
                               [:rust :elle "elle→rust"]
                               [:elle :rust "rust→elle"]]
          (let [label (concat name "[" fs "] count=" (string count))]
            (println "\n── " label " ──")
            (let [[ok? result] (protect (ev/timeout TIMEOUT
                                        (fn []
                                          (let [@server-proc nil
                                            @server-port 0
                                            @server-listener nil
                                            @server-fiber nil]
                                            (if (= st :rust)
                                              (let [[proc port] (start-rust-server DEFAULT-RESPONSE-SIZE
                                                opts)]
                                                (assign server-proc proc)
                                                (assign server-port port))
                                              (let [[listener port fiber] (elle-server 0
                                                DEFAULT-RESPONSE-SIZE opts)]
                                                (assign server-listener listener)
                                                (assign server-port port)
                                                (assign server-fiber fiber)))
                                            (ev/sleep 0.1)
                                            (defer
                                              (begin
                                                (when server-proc
                                                  (protect (subprocess/kill server-proc))
                                                  (protect (subprocess/wait server-proc)))
                                                (when server-listener
                                                  (protect (port/close server-listener)))
                                                (when server-fiber
                                                  (protect (ev/abort server-fiber))))
                                              (if (= ct :rust)
                                                (start-rust-client server-port
                                                count DEFAULT-REQUEST-SIZE
                                                DEFAULT-RESPONSE-SIZE opts)
                                                (elle-client-h10 server-port
                                                count DEFAULT-REQUEST-SIZE
                                                DEFAULT-RESPONSE-SIZE opts)))))))]
              (if ok?
                (if (nil? result)
                  (begin
                    (println "  status=TIMEOUT")
                    (push all-results
                          {:name (concat name "[" fs "]")
                           :count count
                           :pass false
                           :timeout true}))
                  (begin
                    (println "  got=" (or result:messages-received "?")
                             " status=" (if result:pass "PASS" "FAIL"))
                    (push all-results
                          @{:name (concat name "[" fs "]")
                            :count count
                            :pass result:pass
                            :messages-received result:messages-received})))
                (begin
                  (println "  status=ERROR: " result)
                  (push all-results
                        {:name (concat name "[" fs "]")
                         :count count
                         :pass false
                         :error result}))))))))
    (println "\n══ H10 Summary ══")
    (each r in all-results
      (let [status (cond
                     r:timeout "TIMEOUT"
                     r:error "ERROR"
                     r:pass "PASS"
                     true "FAIL")]
        (println "  " r:name " count=" r:count " → " status)))))

## ── H11: artificial decode delay ────────────────────────────────────────
## Slow down the client's message processing to simulate protobuf decode
## latency. If the data-queue backs up, the reader loop blocks, WU stalls.

(defn
  elle-client-h11
  [target-port count request-size response-size opts delay-ms]
  "Elle client with artificial delay per decoded message (H11)."
  (let [url (concat "http://127.0.0.1:" (string target-port))
        session (http2:connect url)
        payload (make-payload request-size)
        gf opts:grpc-framing
        expect-trailers opts:trailers
        start (clock/monotonic)]
    (defer
      (protect (http2:close session))

      (def [sid s]
        (http2:open-stream session "POST" "/echo-amplify"
                           :headers [["x-h2cross" "true"]]))

      (each i in (range 0 count)
        (let [msg (encode-msg payload gf)]
          (http2:stream-send session sid msg)))

      (http2:stream-end session sid)
      (eprintln "[" (ts) "] elle-client[H11]: sent " count " messages, delay="
                delay-ms "ms per decode")

      (def @buf (bytes))
      (def @msg-count 0)
      (def @total-bytes 0)
      (def @done false)
      (def @frame-count 0)

      (while (not done)
        (let [msg (s:data-queue:take)]
          (when (nil? msg) (assign done true))
          (unless (nil? msg)
            (match msg:type
              :headers (when msg:end-stream (assign done true))
              :data
                (begin
                  (assign frame-count (+ frame-count 1))
                  (assign total-bytes (+ total-bytes (length msg:data)))
                  (assign buf (concat buf msg:data))
                  (when msg:end-stream (assign done true)))
              _ (assign done true))))

        (let [[msgs remaining] (decode-msgs buf gf)]
          (each _ in msgs
            (ev/sleep (/ delay-ms 1000.0)))
          (assign msg-count (+ msg-count (length msgs)))
          (assign buf remaining)))

      (let [elapsed (- (clock/monotonic) start)
            pass (= msg-count count)]
        (eprintln "[" (ts) "] elle-client[H11]: done — " msg-count
                  " messages in " (round (* elapsed 1000)) "ms")
        {:messages-sent count
         :messages-received msg-count
         :bytes-received total-bytes
         :expected-bytes 0
         :elapsed-ms (round (* elapsed 1000))
         :pass pass}))))

(defn run-h11 []
  (println "h2cross: H11 — artificial decode delay (1ms per message)")
  (let [counts [384 385]
        opts PLAIN-OPTS
        delay-ms 1
        @results @[]]
    (each [st name] in [[:elle "elle→elle[H11]"] [:rust "elle→rust[H11]"]]
      (each count in counts
        (let [[ok? result] (protect (ev/timeout TIMEOUT
                                    (fn []
                                      (println "\n── " name " count=" count
                                      " ──")
                                      (let [@server-proc nil
                                        @server-port 0
                                        @server-listener nil
                                        @server-fiber nil]
                                        (if (= st :rust)
                                          (let [[proc port] (start-rust-server DEFAULT-RESPONSE-SIZE
                                            opts)]
                                            (assign server-proc proc)
                                            (assign server-port port))
                                          (let [[listener port fiber] (elle-server 0
                                            DEFAULT-RESPONSE-SIZE opts)]
                                            (assign server-listener listener)
                                            (assign server-port port)
                                            (assign server-fiber fiber)))
                                        (ev/sleep 0.1)
                                        (defer
                                          (begin
                                            (when server-proc
                                              (protect (subprocess/kill server-proc))
                                              (protect (subprocess/wait server-proc)))
                                            (when server-listener
                                              (protect (port/close server-listener)))
                                            (when server-fiber
                                              (protect (ev/abort server-fiber))))
                                          (elle-client-h11 server-port count
                                          DEFAULT-REQUEST-SIZE
                                          DEFAULT-RESPONSE-SIZE opts delay-ms))))))]
          (if ok?
            (if (nil? result)
              (begin
                (println "  status=TIMEOUT")
                (push results
                      {:name name :count count :pass false :timeout true}))
              (begin
                (println "  status=" (if result:pass "PASS" "FAIL") " elapsed="
                         result:elapsed-ms "ms")
                (push results
                      @{:name name
                        :count count
                        :pass result:pass
                        :elapsed-ms result:elapsed-ms})))
            (begin
              (println "  status=ERROR: " result)
              (push results {:name name :count count :pass false :error result}))))))
    (println "\n══ H11 Summary ══")
    (each r in results
      (let [status (cond
                     r:timeout "TIMEOUT"
                     r:error "ERROR"
                     r:pass "PASS"
                     true "FAIL")]
        (println "  " r:name " count=" r:count " → " status
                 (if r:elapsed-ms (concat " (" (string r:elapsed-ms) "ms)") ""))))))

## ── H12: buffer accumulation observation ────────────────────────────────
## Same protocol, but client logs buf length at each decode cycle to
## detect if (concat buf msg:data) causes unbounded growth.

(defn elle-client-h12 [target-port count request-size response-size opts]
  "Elle client that logs buffer size at each frame (H12)."
  (let [url (concat "http://127.0.0.1:" (string target-port))
        session (http2:connect url)
        payload (make-payload request-size)
        gf opts:grpc-framing
        start (clock/monotonic)]
    (defer
      (protect (http2:close session))

      (def [sid s]
        (http2:open-stream session "POST" "/echo-amplify"
                           :headers [["x-h2cross" "true"]]))

      (each i in (range 0 count)
        (let [msg (encode-msg payload gf)]
          (http2:stream-send session sid msg)))
      (http2:stream-end session sid)

      (def @buf (bytes))
      (def @msg-count 0)
      (def @done false)
      (def @frame-count 0)
      (def @max-buf-len 0)

      (while (not done)
        (let [msg (s:data-queue:take)]
          (when (nil? msg) (assign done true))
          (unless (nil? msg)
            (match msg:type
              :headers (when msg:end-stream (assign done true))
              :data
                (begin
                  (assign frame-count (+ frame-count 1))
                  (assign buf (concat buf msg:data))
                  (let [bl (length buf)]
                    (when (> bl max-buf-len) (assign max-buf-len bl))
                    (when (= 0 (mod frame-count 64))
                      (eprintln "[" (ts) "] H12: frame #" frame-count " buf=" bl
                                " max=" max-buf-len " msgs=" msg-count)))
                  (when msg:end-stream (assign done true)))
              _ (assign done true))))

        (let [[msgs remaining] (decode-msgs buf gf)]
          (assign msg-count (+ msg-count (length msgs)))
          (assign buf remaining)))

      (let [elapsed (- (clock/monotonic) start)
            pass (= msg-count count)]
        (eprintln "[" (ts) "] H12: done — " msg-count " messages, max-buf="
                  max-buf-len " bytes")
        {:messages-sent count
         :messages-received msg-count
         :bytes-received 0
         :expected-bytes 0
         :elapsed-ms (round (* elapsed 1000))
         :max-buf-len max-buf-len
         :pass pass}))))

(defn run-h12 []
  (println "h2cross: H12 — buffer accumulation observation")
  (let [counts [384 650]
        opts PLAIN-OPTS
        @results @[]]
    (each [st name] in [[:elle "elle→elle[H12]"] [:rust "elle→rust[H12]"]]
      (each count in counts
        (let [[ok? result] (protect (ev/timeout TIMEOUT
                                    (fn []
                                      (println "\n── " name " count=" count
                                      " ──")
                                      (let [@server-proc nil
                                        @server-port 0
                                        @server-listener nil
                                        @server-fiber nil]
                                        (if (= st :rust)
                                          (let [[proc port] (start-rust-server DEFAULT-RESPONSE-SIZE
                                            opts)]
                                            (assign server-proc proc)
                                            (assign server-port port))
                                          (let [[listener port fiber] (elle-server 0
                                            DEFAULT-RESPONSE-SIZE opts)]
                                            (assign server-listener listener)
                                            (assign server-port port)
                                            (assign server-fiber fiber)))
                                        (ev/sleep 0.1)
                                        (defer
                                          (begin
                                            (when server-proc
                                              (protect (subprocess/kill server-proc))
                                              (protect (subprocess/wait server-proc)))
                                            (when server-listener
                                              (protect (port/close server-listener)))
                                            (when server-fiber
                                              (protect (ev/abort server-fiber))))
                                          (elle-client-h12 server-port count
                                          DEFAULT-REQUEST-SIZE
                                          DEFAULT-RESPONSE-SIZE opts))))))]
          (if ok?
            (if (nil? result)
              (begin
                (println "  status=TIMEOUT")
                (push results
                      {:name name :count count :pass false :timeout true}))
              (begin
                (println "  status=" (if result:pass "PASS" "FAIL") " max-buf="
                         result:max-buf-len)
                (push results
                      @{:name name
                        :count count
                        :pass result:pass
                        :max-buf-len result:max-buf-len})))
            (begin
              (println "  status=ERROR: " result)
              (push results {:name name :count count :pass false :error result}))))))
    (println "\n══ H12 Summary ══")
    (each r in results
      (let [status (cond
                     r:timeout "TIMEOUT"
                     r:error "ERROR"
                     r:pass "PASS"
                     true "FAIL")]
        (println "  " r:name " count=" r:count " → " status
                 (if r:max-buf-len
                   (concat " (max-buf=" (string r:max-buf-len) ")")
                   ""))))))

(defn run-one [permutation]
  (let [counts [384 385 650]]
    (match permutation  # Plain permutations
      "elle-elle" (begin
                    (println "h2cross: elle→elle sweep")
                    (run-sweep :elle :elle "elle→elle" counts PLAIN-OPTS))
      "elle-rust" (begin
                    (println "h2cross: elle→rust sweep")
                    (run-sweep :rust :elle "elle→rust" counts PLAIN-OPTS))
      "rust-elle" (begin
                    (println "h2cross: rust→elle sweep")
                    (run-sweep :elle :rust "rust→elle" counts PLAIN-OPTS))
      "rust-rust" (begin
                    (println "h2cross: rust→rust sweep")
                    (run-sweep :rust :rust "rust→rust" counts PLAIN-OPTS))

      # Individual hypotheses — all 4 permutations each
      "h2" (run-hypothesis "H2: gRPC framing"
                           {:grpc-framing true :trailers false :window-size nil})
      "h3" (run-hypothesis "H3: trailers"
                           {:grpc-framing false :trailers true :window-size nil})
      "h4" (run-hypothesis "H4: 64MB windows"
                           {:grpc-framing false
                            :trailers false
                            :window-size 67108864})

      # Combined hypotheses
      "h23" (run-hypothesis "H2+H3: gRPC framing + trailers"
                            {:grpc-framing true :trailers true :window-size nil})
      "h24" (run-hypothesis "H2+H4: gRPC framing + 64MB windows"
                            {:grpc-framing true
                             :trailers false
                             :window-size 67108864})
      "h34" (run-hypothesis "H3+H4: trailers + 64MB windows"
                            {:grpc-framing false
                             :trailers true
                             :window-size 67108864})
      "h234" (run-hypothesis "H2+H3+H4: gRPC framing + trailers + 64MB windows"
                             {:grpc-framing true
                              :trailers true
                              :window-size 67108864})

      # Aliases
      "h134" (run-hypothesis "H1+H3+H4 (streaming+trailers+windows)"
                             {:grpc-framing true
                              :trailers true
                              :window-size 67108864})

      ## ── H5: data-queue saturation (small responses) ──
      ## 50-byte responses → each DATA frame is ~54 bytes (4+50 or 5+50)
      ## data-queue has 64 slots → saturates at 64 frames
      ## 384 = 6 × 64, tests exact queue cycling boundary
      "h5" (run-hypothesis "H5: small responses (queue saturation)" PLAIN-OPTS
                           :response-size 50 :counts [64 128 384 385 650])

      ## ── H5+H2+H3: small gRPC responses with trailers ──
      "h5-grpc" (run-hypothesis "H5+H2+H3: small gRPC responses + trailers"
                                {:grpc-framing true
                                 :trailers true
                                 :window-size nil} :response-size 50
                                :counts [64 128 384 385 650])

      ## ── H6: WU threshold observation ──
      ## Run with standard params; h2-debug output already logs WU timing.
      ## Use stderr to observe WU send/receive timestamps on both sides.
      "h6" (begin
             (println "h2cross: H6 — WU threshold observation")
             (println "  Watch h2-debug WU output on stderr")
             (println "  wu-threshold = 512KB (INITIAL-WINDOW/2)")
             (println "  384 × 12KB = 4.6MB → expect ~9 WU rounds")
             (run-4way "H6: WU observation" PLAIN-OPTS))

      ## ── H7: writer fiber starvation ──
      ## Same as H6 — observable via WU timing gaps in h2-debug output.
      ## If WU frames are delayed, gaps between conn-WU entries will be large.
      "h7" (begin
             (println "h2cross: H7 — writer fiber starvation observation")
             (println "  Look for large timing gaps between WU entries in stderr")
             (run-4way "H7: writer timing" PLAIN-OPTS))

      ## ── H8: concurrent streams sharing connection window ──
      "h8" (run-h8)

      ## ── H9: PING keepalive under load ──
      "h9" (run-h9)

      ## ── H10: connection recycling race ──
      "h10" (run-h10)

      ## ── H11: artificial decode delay in client ──
      "h11" (run-h11)

      ## ── H12: buffer accumulation observation ──
      ## Same protocol, but client logs buf length after each decode cycle.
      "h12" (run-h12)

      _ (error (concat "unknown permutation: " permutation
                       "\n  plain: elle-elle, elle-rust, rust-elle, rust-rust"
                       "\n  hypotheses: h2-h12, h23, h24, h34, h234")))))

# Entry point
(let [args (sys/args)]
  (if (>= (length args) 1) (run-one (get args 0)) (run-all)))
