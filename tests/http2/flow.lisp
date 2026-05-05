(elle/epoch 10)
## tests/http2/flow.lisp — h2 flow control, GOAWAY, and protocol tests

(def sync ((import "std/sync")))
(def frame ((import "std/http2/frame")))
(def stream ((import "std/http2/stream") :sync sync :frame frame))
(def hpack ((import "std/http2/hpack") :huffman ((import "std/http2/huffman"))))
(def http2 ((import "std/http2")))
(def C frame:constants)

## ── Helpers ──────────────────────────────────────────────────────────────

(defn listen-ephemeral []
  (let* [listener (tcp/listen "127.0.0.1" 0)
         lpath (port/path listener)
         lport (parse-int (slice lpath (+ 1 (string/find lpath ":"))))]
    [listener lport]))

(defn make-raw-transport [tcp-port]
  {:read (fn [n] (port/read tcp-port n))
   :write (fn [data] (port/write tcp-port (if (bytes? data) data (bytes data))))
   :flush (fn [] nil)
   :close (fn [] (port/close tcp-port))})

(defn server-handshake [t]
  (frame:read-exact t 24)
  (frame:read-frame t 16384)
  (let [[ft fl si pl] (frame:make-settings-frame [])]
    (frame:write-frame t ft fl si pl))
  (let [[ft fl si pl] (frame:make-settings-ack)]
    (frame:write-frame t ft fl si pl))
  (t:flush))

(defn server-handshake-with-settings [t settings]
  "Server handshake with custom SETTINGS."
  (frame:read-exact t 24)
  (frame:read-frame t 16384)
  (let [[ft fl si pl] (frame:make-settings-frame settings)]
    (frame:write-frame t ft fl si pl))
  (let [[ft fl si pl] (frame:make-settings-ack)]
    (frame:write-frame t ft fl si pl))
  (t:flush))

(defn drain-control-frames [t]
  "Read and discard SETTINGS/WINDOW_UPDATE frames until something else arrives."
  (forever
    (let [[ok? f] (protect (frame:read-frame t 262144))]
      (when (or (not ok?) (nil? f)) (break nil))
      (cond
        (= f:type C:type-settings) nil
        (= f:type C:type-window-update) nil
        (= f:type C:type-goaway) (break f)
        true (break f)))))

## ── Tests ────────────────────────────────────────────────────────────────

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

(defn test-reader-death-notification []
  (let* [[listener lport] (listen-ephemeral)
         sf (ev/spawn (fn []
                        (let [t (make-raw-transport (tcp/accept listener))]
                          (server-handshake t)
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

## ── New tests ──────────────────────────────────────────────────────────

(defn test-goaway-from-raw-server []
  "Raw server sends GOAWAY; client should see goaway-recvd? set."
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
                                  (begin  # Respond, then send GOAWAY
                                    (let* [resp-hdr (hpack:encode enc
                                      [[":status" "200"]])
                                      [ft fl si pl] (frame:make-headers-frame f:stream-id
                                      resp-hdr true true)]
                                      (frame:write-frame t ft fl si pl))
                                    (let [[ft fl si pl] (frame:make-goaway-frame f:stream-id
                                      C:err-no-error)]
                                      (frame:write-frame t ft fl si pl))
                                    (t:flush))
                                true nil)))
                          (protect (t:close)))))]
    (let [session (http2:connect (concat "http://127.0.0.1:" (string lport)))]
      (defer
        (begin
          (protect (http2:close session))
          (protect (ev/join sf)))
        (let [resp (http2:send session "GET" "/test")]
          (assert (= resp:status 200) "goaway: first request ok"))  # Give reader time to process the GOAWAY
        (ev/sleep 0.1)
        (assert session:goaway-recvd? "goaway: goaway-recvd? set")
        (let [[ok? _] (protect (http2:send session "GET" "/nope"))]
          (assert (not ok?) "goaway: second request refused")))))
  (println "  PASS: GOAWAY from raw server"))

(defn test-concurrent-requests []
  "5 parallel requests on one session all complete."
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
                                    (let* [body (bytes (concat "resp-"
                                      (string f:stream-id)))
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
        (let [fibers (map (fn [i]
                            (ev/spawn (fn []
                                        (http2:send session "GET"
                                        (concat "/concurrent-" (string i))))))
                          (range 0 5))
              results (map ev/join fibers)]
          (each r in results
            (assert (= r:status 200) "concurrent: status 200"))
          (assert (= (length results) 5) "concurrent: 5 results")))))
  (println "  PASS: concurrent requests"))

(defn test-settings-window-adjustment-e2e []
  "Raw server sends SETTINGS changing INITIAL_WINDOW_SIZE mid-session."
  (let* [[listener lport] (listen-ephemeral)
         enc (hpack:make-encoder :use-huffman false)
         sf (ev/spawn (fn []
                        (let [t (make-raw-transport (tcp/accept listener))]
                          (server-handshake t)  # Wait for client SETTINGS ACK, then send our own SETTINGS
                          # changing INITIAL_WINDOW_SIZE
                          (ev/sleep 0.1)
                          (let [payload (concat (frame:u16->bytes C:settings-initial-window-size)
                                (frame:u32->bytes 32768))
                                [ft fl si pl] (frame:make-settings-frame [[C:settings-initial-window-size
                                32768]])]
                            (frame:write-frame t ft fl si pl))
                          (t:flush)  # Read frames until done
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
                                      resp-hdr true true)]
                                      (frame:write-frame t ft fl si pl))
                                    (t:flush))
                                true nil)))
                          (protect (t:close)))))]
    (let [session (http2:connect (concat "http://127.0.0.1:" (string lport)))]
      (defer
        (begin
          (protect (http2:close session))
          (protect (ev/join sf)))  # Poll until SETTINGS applied (up to 3s)
        (let [@attempts 0]
          (while (and (not (= (get session:remote-settings :initial-window-size)
                              32768)) (< attempts 30))
            (ev/sleep 0.1)
            (assign attempts (+ attempts 1))))
        (assert (= (get session:remote-settings :initial-window-size) 32768)
                (concat "settings: window adjusted to 32768, got "
                        (string (get session:remote-settings
                                     :initial-window-size))))  # Verify we can still send
        (let [resp (http2:send session "GET" "/after-settings")]
          (assert (= resp:status 200) "settings: request after adjustment ok")))))
  (println "  PASS: SETTINGS window adjustment e2e"))

## ── Run ──────────────────────────────────────────────────────────────────

(println "h2 flow control tests:")
(test-conn-flow-initial)
(test-reader-death-notification)
(test-many-sequential-streams)
(test-goaway-from-raw-server)
(test-concurrent-requests)
(test-settings-window-adjustment-e2e)
(println "all h2 flow control tests passed")
(sys/exit 0)
