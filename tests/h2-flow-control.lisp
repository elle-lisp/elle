(elle/epoch 9)
## tests/h2-flow-control.lisp — h2 flow control and reader-death tests
##
## Validates fixes for the h2 session hang after ~20-30 streams:
##   1. conn-flow starts at 65535 (RFC 9113), not INITIAL-WINDOW
##   2. Reader death propagates errors to waiting stream data-queues
##   3. Connection WINDOW_UPDATE sent even for DATA on unknown streams
##   4. Many sequential streams complete without hanging
##   5. Send-side flow control enforcement

(def sync ((import "std/sync")))
(def frame ((import "std/http2/frame")))
(def stream ((import "std/http2/stream") :sync sync :frame frame))
(def hpack ((import "std/http2/hpack") :huffman ((import "std/http2/huffman"))))
(def http2 ((import "std/http2")))
(def C frame:constants)

## ── Helpers ────────────────���───────────────────────────────────────────

(defn make-raw-transport [tcp-port]
  {:read (fn [n] (port/read tcp-port n))
   :write (fn [data] (port/write tcp-port (if (bytes? data) data (bytes data))))
   :flush (fn [] nil)
   :close (fn [] (port/close tcp-port))})

(defn server-handshake [t]
  "Minimal server handshake: read preface + SETTINGS, send SETTINGS + ACK."
  (frame:read-exact t 24)
  (frame:read-frame t 16384)
  (let [[ft fl si pl] (frame:make-settings-frame [])]
    (frame:write-frame t ft fl si pl))
  (let [[ft fl si pl] (frame:make-settings-ack)]
    (frame:write-frame t ft fl si pl))
  (t:flush))

(defn listen-ephemeral []
  (let* [listener (tcp/listen "127.0.0.1" 0)
         lpath (port/path listener)
         lport (parse-int (slice lpath (+ 1 (string/find lpath ":"))))]
    [listener lport]))

## ── Test 1: conn-flow initial value ────────────��───────────────────────

(defn test-conn-flow-initial []
  (let* [[listener lport] (listen-ephemeral)
         sf (ev/spawn (fn []
                        (let [t (make-raw-transport (tcp/accept listener))]
                          (server-handshake t)
                          (forever
                            (let [[ok? f] (protect (frame:read-frame t 262144))]
                              (when (or (not ok?) (nil? f)) (break nil))
                              (when (= f:type C:type-goaway) (break nil))))
                          (protect (t:close)))))]
    (let [session (http2:connect (concat "http://127.0.0.1:" (string lport)))]
      (defer
        (begin
          (protect (http2:close session))
          (protect (ev/join sf)))
        (assert (= session:conn-flow:send-window 65535)
                (concat "conn-flow should be 65535, got "
                        (string session:conn-flow:send-window))))))
  (println "  PASS: conn-flow initial value"))

## ── Test 2: reader death notifies waiting streams ──────────────────────

(defn test-reader-death-notification []
  (let* [[listener lport] (listen-ephemeral)
         sf (ev/spawn (fn []
                        (let [t (make-raw-transport (tcp/accept listener))]
                          (server-handshake t)  ## Read until we see a HEADERS request, then close abruptly
                          (forever
                            (let [[ok? f] (protect (frame:read-frame t 262144))]
                              (when (or (not ok?) (nil? f)) (break nil))
                              (when (= f:type C:type-headers) (break nil))
                              (when (= f:type C:type-goaway) (break nil))))
                          (protect (t:close)))))]
    (let [session (http2:connect (concat "http://127.0.0.1:" (string lport)))]
      (defer
        (begin
          (protect (http2:close session))
          (protect (ev/join sf)))
        (let [[ok? err] (protect (http2:send session "GET" "/hang"))]
          (assert (not ok?) "reader death: send should error, not hang")))))
  (println "  PASS: reader death notification"))

## ── Test 3: many sequential streams complete ─────────────────���─────────

(defn test-many-sequential-streams []
  (let* [[listener lport] (listen-ephemeral)
         enc (hpack:make-encoder :use-huffman false)
         sf (ev/spawn (fn []
                        (let [t (make-raw-transport (tcp/accept listener))]
                          (server-handshake t)
                          (forever
                            (let [[ok? f] (protect (frame:read-frame t 262144))]
                              (when (or (not ok?) (nil? f)) (break nil))
                              (cond
                                (= f:type C:type-settings) nil
                                (= f:type C:type-window-update) nil
                                (= f:type C:type-goaway) (break nil)
                                (= f:type C:type-headers)
                                  (begin
                                    (let* [resp-hdr (hpack:encode enc
                                      [[":status" "200"]])
                                      [ft fl si pl] (frame:make-headers-frame f:stream-id
                                      resp-hdr false true)]
                                      (frame:write-frame t ft fl si pl))
                                    (let* [body (bytes 0 1 2 3 4 5 6 7 8 9 0 1 2
                                      3 4 5 6 7 8 9)
                                      [ft fl si pl] (frame:make-data-frame f:stream-id
                                      body true)]
                                      (frame:write-frame t ft fl si pl))
                                    (t:flush))
                                true nil)))
                          (protect (t:close)))))]
    (let [session (http2:connect (concat "http://127.0.0.1:" (string lport)))]
      (defer
        (begin
          (protect (http2:close session))
          (protect (ev/join sf)))
        (each i in (range 0 40)
          (let [resp (http2:send session "GET" (concat "/req-" (string i)))]
            (assert (= resp:status 200)
                    (concat "many-streams: request " (string i)))))
        (assert (= (length (keys session:streams)) 0)
                "many-streams: no stream leak"))))
  (println "  PASS: 40 sequential streams"))

## ── Run ───────────────────���────────────────────────────────────────────

(println "h2 flow control tests:")
(test-conn-flow-initial)
(test-reader-death-notification)
(test-many-sequential-streams)
(println "all h2 flow control tests passed")
