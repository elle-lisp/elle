(elle/epoch 10)
## tests/elle/websocket.lisp — WebSocket module tests


## ── Plugin availability check ────────────────────────────────────

(let [[h-ok? _] (protect (import "plugin/hash"))
      [r-ok? _] (protect (import "plugin/random"))]
  (unless (and h-ok? r-ok?)
    (println "SKIP: plugin/hash or plugin/random not available")
    (exit 0)))

## ── Init ─────────────────────────────────────────────────────────

(def hash-plug (import "plugin/hash"))
(def rand-plug (import "plugin/random"))
(def ws ((import "std/websocket") :hash hash-plug :random rand-plug))

## ── Internal pure tests ──────────────────────────────────────────

(ws:test)

## ── URL parsing ──────────────────────────────────────────────────

(let [p (ws:parse-url "ws://localhost:9090/path")]
  (assert (= p:scheme "ws") "url: ws scheme")
  (assert (= p:host "localhost") "url: host")
  (assert (= p:port 9090) "url: explicit port")
  (assert (= p:path "/path") "url: path"))

(let [p (ws:parse-url "ws://example.com")]
  (assert (= p:port 80) "url: ws default 80"))

(let [p (ws:parse-url "wss://example.com")]
  (assert (= p:port 443) "url: wss default 443"))

(let [[ok? _] (protect (ws:parse-url "http://example.com"))]
  (assert (not ok?) "url: rejects http://"))

## ── Loopback integration test ────────────────────────────────────

(defn make-t [tcp]
  "Transport wrapper for a raw TCP port."
  (def @wbuf @[])
  {:read (fn [n] (port/read tcp n))
   :read-line (fn [] (port/read-line tcp))
   :write (fn [data]
            (let [d (if (bytes? data) data (bytes data))]
              (push wbuf d)))
   :flush (fn []
            (when (> (length wbuf) 0)
              (let [combined (apply concat (freeze wbuf))]
                (port/write tcp combined)
                (assign wbuf @[]))))
   :close (fn [] (port/close tcp))})

(let* [listener (tcp/listen "127.0.0.1" 0)
       lpath (port/path listener)
       lport (parse-int (slice lpath (+ 1 (string/find lpath ":"))))
       _srv (ev/spawn (fn []
                        (let* [tcp (tcp/accept listener)
                               t (make-t tcp)]
                          (def line (t:read-line))
                          (def parts (string/split line " "))
                          (def @hdrs @{})
                          (forever
                            (let [h (t:read-line)]
                              (when (or (nil? h) (empty? h) (= h "\r"))
                                (break nil))
                              (let [c (string/find h ":")]
                                (when c
                                  (put hdrs
                                       (keyword (string/lowercase (string/trim (slice h
                                       0 c)))) (string/trim (slice h (+ c 1))))))))
                          (let* [req {:method (get parts 0)
                                      :path (get parts 1)
                                      :headers (freeze hdrs)
                                      :body nil}
                                 conn (ws:upgrade req t)]
                            (forever
                              (let [msg (ws:recv conn)]
                                (cond
                                  (= msg:type :close) (begin
                                    (ws:close conn)
                                    (break nil))
                                  (= msg:type :text) (ws:send conn
                                  (string msg:data))
                                  (= msg:type :binary) (ws:send conn msg:data))))))))]
  (let* [url (string "ws://127.0.0.1:" lport "/echo")
         conn (ws:connect url)]
    (ws:send conn "hello websocket")
    (let [msg (ws:recv conn)]
      (assert (= msg:type :text) "loopback: text type")
      (assert (= (string msg:data) "hello websocket") "loopback: text echo"))  ## Binary echo
    (ws:send conn (bytes 1 2 3 4 5))
    (let [msg (ws:recv conn)]
      (assert (= msg:type :binary) "loopback: binary type")
      (assert (= msg:data (bytes 1 2 3 4 5)) "loopback: binary echo"))  ## Close — server will echo close frame
    (ws:close conn))
  (port/close listener))

## ── Error path: connection refused ───────────────────────────────

(let [[ok? _] (protect (ws:connect "ws://127.0.0.1:1/nope"))]
  (assert (not ok?) "connect refused on port 1"))

(println "tests/elle/websocket.lisp: all tests passed")
