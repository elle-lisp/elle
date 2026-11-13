(elle/epoch 15)
# audited: 2026-11-12
# The WebSocket module parses URLs, talks to a loopback server, keeps two sending fibers' messages apart, and reports a refused connection.
# docs/libraries.md
#

## ── Plugin availability check ────────────────────────────────────

(let [[h-ok? _] (protect (import "plugin/hash"))
      [r-ok? _] (protect (import "plugin/random"))]
  (unless (and h-ok? r-ok?)
    (error (struct :error :gated
                   :reason "plugin/hash or plugin/random not available"))))

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
  {:read (fn [n] (port/read tcp n))
   :read-line (fn []
                (let [line (port/read-line tcp)]
                  (when line (string line))))
   :write (fn [data] (port/write tcp data))
   :flush (fn [] nil)
   :close (fn [] (port/close tcp))})

(defn read-request [t]
  "The request line and headers a client sent on transport t, as the request
   ws:upgrade takes."
  (let* [line (t:read-line)
         parts (string/split line " ")
         hdrs @{}]
    (forever
      (let [h (t:read-line)]
        (when (or (nil? h) (empty? h) (= h "\r")) (break nil))
        (let [c (string/find h ":")]
          (when c
            (put hdrs (keyword (string/lowercase (string/trim (slice h 0 c))))
                 (string/trim (slice h (+ c 1))))))))
    {:method (get parts 0) :path (get parts 1) :headers (freeze hdrs) :body nil}))

(let* [listener (tcp/listen "127.0.0.1" 0)
       lpath (port/path listener)
       lport (parse-int (slice lpath (+ 1 (string/find lpath ":"))))
       _srv (ev/spawn (fn []
                        (let* [tcp (tcp/accept listener)
                               t (make-t tcp)]
                          (let* [conn (ws:upgrade (read-request t) t)]
                            (forever
                              (let [msg (ws:recv conn)]
                                (cond
                                  (= msg:type :close)
                                    (begin
                                      (ws:close conn)
                                      (break nil))
                                  (= msg:type :text)
                                    (ws:send conn (string msg:data))
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

## ── Two fibers send on one connection ────────────────────────────
# A send that yields inside its write lets the other fiber's send run. A
# transport that empties its write buffer only after that write sends the
# first message twice, and the server hears three messages.

(let* [sync ((import "std/sync"))
       listener (tcp/listen "127.0.0.1" 0)
       lpath (port/path listener)
       lport (parse-int (slice lpath (+ 1 (string/find lpath ":"))))
       heard @[]
       ended (sync:make-latch)]
  (ev/spawn (fn []
              (ws:serve listener
                        (fn [conn]
                          (forever
                            (let [msg (ws:recv conn)]
                              (when (or (= msg:type :close)
                                        (= (string msg:data) "end"))
                                (break nil))
                              (push heard (string msg:data))))
                          (ended:open)))))
  (let [conn (ws:connect (string "ws://127.0.0.1:" lport "/two"))]
    (ev/join [(ev/spawn (fn [] (ws:send conn "first")))
              (ev/spawn (fn [] (ws:send conn "second")))])
    (ws:send conn "end")
    (ended:wait)
    (assert (= (sort (freeze heard)) ["first" "second"])
            "two senders: each message arrives once")
    (ws:close conn))
  (port/close listener))

## ── Two fibers send large messages on one connection ─────────────
# The server accepts with a 4 KiB send buffer, so each send of a 1 MiB
# message takes hundreds of write(2) calls. A port that let the second write
# start before the first ended would mix their bytes, and the client would
# read a frame header out of the middle of a payload. The server sends, so no
# frame is masked. A default send buffer takes most of such a message in one
# call, and a mixed port passes then. The transport gathers each frame into
# one write, as the one std/websocket builds over TCP does.

(defn gather-t [tcp]
  "A transport over tcp whose flush sends the queued parts in one write."
  (let [framing ((import "std/framing"))
        g (framing:gather tcp)]
    {:read (fn [n] (port/read tcp n))
     :read-line (fn []
                  (let [line (port/read-line tcp)]
                    (when line (string line))))
     :write (fn [data] (framing:write g data))
     :flush (fn [] (framing:flush g))
     :close (fn [] (port/close tcp))}))

(let* [listener (tcp/listen "127.0.0.1" 0)
       lpath (port/path listener)
       lport (parse-int (slice lpath (+ 1 (string/find lpath ":"))))
       size (* 1024 1024)
       payloads [(bytes (string/repeat "a" size))
                 (bytes (string/repeat "b" size))]]
  (ev/spawn (fn []
              (let* [tcp (tcp/accept listener :sndbuf 4096)
                     t (gather-t tcp)
                     conn (ws:upgrade (read-request t) t)]
                (ev/join (map (fn [p] (ev/spawn (fn [] (ws:send conn p))))
                              payloads))
                (ws:recv conn)
                (port/close tcp))))
  (let* [conn (ws:connect (string "ws://127.0.0.1:" lport "/big"))
         got (ev/timeout 10 (fn [] [(ws:recv conn) (ws:recv conn)]))]
    (assert (not (nil? got)) "two large senders: both messages arrive in time")
    (let [[one two] got
          which (fn [msg] (find-index (fn [p] (= p msg:data)) payloads))]
      (assert (and (= one:type :binary) (= two:type :binary))
              "two large senders: each message is binary")
      (assert (not (nil? (which one)))
              "two large senders: the first arrives whole")
      (assert (not (nil? (which two)))
              "two large senders: the second arrives whole")
      (assert (not (= (which one) (which two)))
              "two large senders: each message arrives once"))
    (ws:close conn))
  (port/close listener))

## ── Error path: connection refused ───────────────────────────────

(let [[ok? _] (protect (ws:connect "ws://127.0.0.1:1/nope"))]
  (assert (not ok?) "connect refused on port 1"))

(println "tests/lang/websocket.lisp: all tests passed")
