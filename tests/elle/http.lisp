# HTTP module tests
#
# Tests the public API of lib/http.lisp. Internal wire-format helpers
# are tested via (http:test) which runs sanity checks inside the module.

(def http ((import "std/http")))

# ============================================================================
# Internal wire-format sanity checks
# ============================================================================

(http:test)

# ============================================================================
# URL parsing (pure, no I/O)
# ============================================================================

# Full URL with all components
(let [[u (http:parse-url "http://example.com:8080/api/users?page=2")]]
  (assert (= u:scheme "http")        "full url: scheme")
  (assert (= u:host   "example.com") "full url: host")
  (assert (= u:port   8080)          "full url: port")
  (assert (= u:path   "/api/users")  "full url: path")
  (assert (= u:query  "page=2")      "full url: query"))

# Default port (80)
(let [[u (http:parse-url "http://example.com/index.html")]]
  (assert (= u:port 80)             "default port is 80")
  (assert (= u:path "/index.html")  "path with default port"))

# No path defaults to "/"
(let [[u (http:parse-url "http://example.com")]]
  (assert (= u:path "/")  "no path defaults to /")
  (assert (nil? u:query)   "no query is nil"))

# Query string present, no path
(let [[u (http:parse-url "http://example.com/?q=hello")]]
  (assert (= u:path  "/")       "path is / with query")
  (assert (= u:query "q=hello") "query parsed"))

# Trailing slash, no query
(let [[u (http:parse-url "http://localhost:3000/")]]
  (assert (= u:host "localhost") "localhost host")
  (assert (= u:port 3000)       "port 3000")
  (assert (= u:path "/")        "path /")
  (assert (nil? u:query)         "no query"))

# Error: malformed (no scheme)
(let [[[ok? err] (protect (http:parse-url "example.com/foo"))]]
  (assert (not ok?)                        "bare hostname signals error")
  (assert (= (get err :error) :http-error) "bare hostname is :http-error"))

# HTTPS: default port 443
(let [[u (http:parse-url "https://example.com/")]]
  (assert (= u:scheme "https")       "https scheme")
  (assert (= u:host   "example.com") "https host")
  (assert (= u:port   443)            "https default port 443")
  (assert (= u:path   "/")            "https path"))

# HTTPS: explicit port, path, query
(let [[u (http:parse-url "https://api.example.com:8443/v1/items?limit=10")]]
  (assert (= u:scheme "https")            "https scheme")
  (assert (= u:host   "api.example.com")  "https host")
  (assert (= u:port   8443)               "https explicit port")
  (assert (= u:path   "/v1/items")        "https path")
  (assert (= u:query  "limit=10")         "https query"))

# HTTPS: no path defaults to /
(let [[u (http:parse-url "https://example.com")]]
  (assert (= u:port 443) "https no path: default port")
  (assert (= u:path "/") "https no path: default /"))

# Error: non-http/https scheme
(let [[[ok? err] (protect (http:parse-url "ftp://example.com/"))]]
  (assert (not ok?)                        "ftp scheme signals error")
  (assert (= (get err :error) :http-error) "ftp scheme is :http-error"))
(let [[[ok? err] (protect (http:parse-url "wss://example.com/"))]]
  (assert (not ok?)                        "wss scheme signals error")
  (assert (= (get err :error) :http-error) "wss scheme is :http-error"))

# ============================================================================
# Response construction (pure, no I/O)
# ============================================================================

(let [[r (http:respond 200 "hello")]]
  (assert (= r:status 200)                              "respond: status")
  (assert (= r:body "hello")                            "respond: body")
  (assert (= (get r:headers :content-length) "5")       "respond: content-length")
  (assert (= (get r:headers :content-type) "text/plain") "respond: content-type"))

# Custom headers override defaults
(let [[r (http:respond 200 "data" :headers {:content-type "application/json"})]]
  (assert (= (get r:headers :content-type) "application/json")
    "respond: custom content-type overrides default"))

# ============================================================================
# Client API — error paths
# ============================================================================

# Connection refused (nothing listening on port 1)
(let [[[ok? _] (protect (http:get "http://127.0.0.1:1/"))]]
  (assert (not ok?) "http:get connection refused signals error"))

# ============================================================================
# TLS plugin integration — https requires :tls on module init
# ============================================================================

# Without :tls, https URLs must fail with a clear :tls-not-configured error.
(let [[[ok? err] (protect (http:get "https://example.com/"))]]
  (assert (not ok?)                           "https without :tls signals error")
  (assert (= err:reason :tls-not-configured)
    "https without :tls reports :tls-not-configured"))

# With :tls, the module uses the supplied plugin. We use a fake plugin that
# records calls so we can verify the wiring without pulling in real TLS.
(def tls-log @[])
(defn push-log [& args] (push tls-log args))

(def fake-tls
  {:connect   (fn [host port] (push-log :connect host port) :fake-conn)
   :read      (fn [conn n]   (push-log :read conn n) nil)
   :read-line (fn [conn]     (push-log :read-line conn) nil)
   :write     (fn [conn data] (push-log :write conn data) (length data))
   :close     (fn [conn]     (push-log :close conn))})

(def https-http ((import "std/http") :tls fake-tls))

# The fake TLS plugin returns nil from read-line, which the wire-format
# code treats as EOF and surfaces as malformed input. That's fine — we
# just want to confirm the TLS path is taken, not that the full handshake
# succeeds.
(let [[[ok? _] (protect (https-http:get "https://example.com/"))]]
  (assert (not ok?) "https-http:get exits via TLS path"))

(assert (= (first (first tls-log)) :connect)
  "tls:connect was called via https URL")
(assert (= (get (first tls-log) 1) "example.com")
  "tls:connect received the https host")
(assert (= (get (first tls-log) 2) 443)
  "tls:connect received the default https port 443")

# Parity: tls-transport's read-line must strip trailing newlines so the
# wire-format helpers behave identically whether the underlying pipe is
# TCP or TLS. We simulate a full HTTP/1.1 response from a TLS peer that
# emits lines with CRLF intact (which is what the real tls:read-line
# does), and verify the parser doesn't choke.

(def canned-response
  ["HTTP/1.1 200 OK\r\n"
   "Content-Type: text/plain\r\n"
   "Content-Length: 5\r\n"
   "\r\n"])
(def canned-cursor @[0])

(defn canned-read-line [conn]
  "Fake tls:read-line: returns the next response line with CRLF intact."
  (let [[i (get canned-cursor 0)]]
    (put canned-cursor 0 (inc i))
    (when (< i (length canned-response))
      (get canned-response i))))

(def canned-body-cursor @[0])

(defn canned-read [conn n]
  "Fake tls:read: dribble out the body one chunk at a time."
  (let [[i (get canned-body-cursor 0)]
        [body "hello"]]
    (when (< i (length body))
      (let [[end (min (length body) (+ i n))]]
        (put canned-body-cursor 0 end)
        (bytes (slice body i end))))))

(def line-strip-tls
  {:connect   (fn [host port] :canned)
   :read      canned-read
   :read-line canned-read-line
   :write     (fn [conn data] (length data))
   :close     (fn [conn] nil)})

(def line-strip-http ((import "std/http") :tls line-strip-tls))
(let [[resp (line-strip-http:get "https://example.com/")]]
  (assert (= resp:status 200)
    "tls-transport: read-line stripping yields parseable status")
  (assert (= resp:body "hello")
    "tls-transport: full https response round-trips"))

# ============================================================================
# :compress — raw helpers exposed via http module
# ============================================================================

# Without :compress, http:gzip signals :compress-not-configured.
(let [[[ok? err] (protect (http:gzip "hello"))]]
  (assert (not ok?)                        "no :compress ⇒ http:gzip errors")
  (assert (= err:reason :compress-not-configured)
    "no :compress ⇒ :compress-not-configured reason"))

# Fake compress plugin: just record calls and return a tagged value so
# we don't depend on libz being present in the test environment.
(def compress-log @[])
(def fake-compress
  {:gzip    (fn [data & opts] (push compress-log [:gzip data opts])    (bytes (string "GZ:" data)))
   :gunzip  (fn [data]         (push compress-log [:gunzip data])       (bytes "ungz"))
   :zlib    (fn [data & opts] (push compress-log [:zlib data opts])    (bytes (string "ZL:" data)))
   :unzlib  (fn [data]         (push compress-log [:unzlib data])       (bytes "unzl"))
   :deflate (fn [data & opts] (push compress-log [:deflate data opts]) (bytes (string "DF:" data)))
   :inflate (fn [data]         (push compress-log [:inflate data])      (bytes "infl"))
   :zstd    (fn [data & opts] (push compress-log [:zstd data opts])    (bytes (string "ZD:" data)))
   :unzstd  (fn [data]         (push compress-log [:unzstd data])       (bytes "unzd"))})

(def http-z ((import "std/http") :compress fake-compress))

(assert (= (string (http-z:gzip "hi")) "GZ:hi")
  ":compress struct ⇒ http:gzip dispatches to the plugin")
(assert (= (string (http-z:gunzip (bytes "anything"))) "ungz")
  ":compress struct ⇒ http:gunzip dispatches to the plugin")
(assert (= (string (http-z:zstd "hi" 5)) "ZD:hi")
  ":compress struct ⇒ http:zstd forwards level argument")
(let [[gzip-call (first compress-log)]]
  (assert (= (first gzip-call) :gzip)     "compress-log recorded :gzip call")
  (assert (= (get gzip-call 1) "hi")      "compress-log recorded the data"))

# :compress true ⇒ module imports std/compress itself. Only meaningful
# when libz/libzstd are available — on macOS the FFI loader expects
# libz.dylib, which lib/compress.lisp doesn't currently probe for.
# Mirror the skip guard from tests/elle/compress.lisp.
(let [[[libz-ok? _] (protect ((fn [] (ffi/native "libz.so"))))]
      [[zstd-ok? _] (protect ((fn [] (ffi/native "libzstd.so"))))]]
  (if (and libz-ok? zstd-ok?)
    (let [[http-auto ((import "std/http") :compress true)]]
      (let* [[data (bytes 0x1f 0x8b 0x08 0x00 0x00 0x00 0x00 0x00 0x00 0x03)]
             [[ok? err] (protect (http-auto:gunzip data))]]
        (when (not ok?)
          (assert (not (= err:reason :compress-not-configured))
            ":compress true ⇒ module imported, not reporting compress-not-configured"))))
    (println "SKIP: :compress true test — libz/libzstd not available")))

# Invalid :compress value → clear error at module init
(let [[[ok? err] (protect ((import "std/http") :compress 42))]]
  (assert (not ok?)                    "invalid :compress signals error")
  (assert (= err:reason :bad-compress) "invalid :compress reason"))

# ============================================================================
# Server + Client integration (local loopback)
# ============================================================================

# Bind to ephemeral port on loopback
(def listener (tcp/listen "127.0.0.1" 0))
(def server-addr (port/path listener))
(def server-port
  (let [[parts (string/split server-addr ":")]]
    (parse-int (get parts (- (length parts) 1)))))

# Simple handler: echo request method and path as the body
(defn test-handler [req]
  (http:respond 200 (string req:method " " req:path)))

# Spawn server
(def server-fiber
  (ev/spawn (fn [] (http:serve listener test-handler))))

# Test 1: GET request
(let [[resp (http:get (string "http://127.0.0.1:" server-port "/hello"))]]
  (assert (= resp:status 200) "loopback GET: status 200")
  (assert (= resp:body "GET /hello") "loopback GET: body echoes method+path"))

# Test 2: POST request with body
(let [[resp (http:post (string "http://127.0.0.1:" server-port "/submit") "payload")]]
  (assert (= resp:status 200) "loopback POST: status 200")
  (assert (= resp:body "POST /submit") "loopback POST: body echoes method+path"))

# Test 3: Custom headers round-trip
(let [[resp (http:get (string "http://127.0.0.1:" server-port "/")
              :headers {:x-test "custom-value"})]]
  (assert (= resp:status 200) "loopback custom header: status 200"))

# Test 4: :query encodes a struct into the request path. Elle struct
# iteration is key-sorted, so we assert on the sorted order the server
# will actually receive.
(let [[resp (http:get (string "http://127.0.0.1:" server-port "/search")
              :query {:q "hello world" :page 2})]]
  (assert (= resp:status 200) "query struct: status 200")
  (assert (string/contains? resp:body "/search?page=2&q=hello%20world")
    "query struct: encoded path echoed in body"))

# Test 5: :query merges with existing URL query (URL first, struct after)
(let [[resp (http:get (string "http://127.0.0.1:" server-port "/feed?fmt=json")
              :query {:limit 10})]]
  (assert (string/contains? resp:body "/feed?fmt=json&limit=10")
    "query merge: url query retained, struct appended"))

# Shut down: closing the listener cancels the pending accept, server exits
(port/close listener)

# ============================================================================
# Chunked transfer integration: server streams response, client decodes
# ============================================================================

(def chunk-listener (tcp/listen "127.0.0.1" 0))
(def chunk-port
  (let [[parts (string/split (port/path chunk-listener) ":")]]
    (parse-int (get parts (- (length parts) 1)))))

# Two handlers, dispatched on path:
#  /string — body is a plain string, framed as a single chunk
#  /stream — body is a closure that emits multiple chunks
(defn chunked-handler [req]
  (case req:path
    "/string"
    {:status 200
     :headers {:content-type      "text/plain"
               :transfer-encoding "chunked"}
     :body "single-chunk body"}

    "/stream"
    {:status 200
     :headers {:content-type      "text/plain"
               :transfer-encoding "chunked"}
     :body (fn [write]
             (write "alpha ")
             (write "beta ")
             (write "gamma"))}

    (http:respond 404 "not found")))

(def chunk-fiber
  (ev/spawn (fn [] (http:serve chunk-listener chunked-handler))))

# Single-chunk body
(let [[resp (http:get (string "http://127.0.0.1:" chunk-port "/string"))]]
  (assert (= resp:status 200)               "chunked single: status 200")
  (assert (= resp:body "single-chunk body") "chunked single: body reassembled")
  (assert (= (string/lowercase (get resp:headers :transfer-encoding))
             "chunked")
    "chunked single: server sent Transfer-Encoding: chunked"))

# Streamed multi-chunk body
(let [[resp (http:get (string "http://127.0.0.1:" chunk-port "/stream"))]]
  (assert (= resp:status 200)              "chunked stream: status 200")
  (assert (= resp:body "alpha beta gamma") "chunked stream: chunks concatenated"))

(port/close chunk-listener)

# ============================================================================
# Redirect following
# ============================================================================

(def redir-listener (tcp/listen "127.0.0.1" 0))
(def redir-port
  (let [[parts (string/split (port/path redir-listener) ":")]]
    (parse-int (get parts (- (length parts) 1)))))

(def redir-count @[0])

(defn redir-handler [req]
  (put redir-count 0 (+ (get redir-count 0) 1))
  (case req:path
    # /hop1 → /hop2 (302, rewrites to GET)
    "/hop1"
    {:status 302
     :headers {:location "/hop2" :content-length "0"}
     :body ""}

    # /hop2 → /hop3 (301, rewrites to GET)
    "/hop2"
    {:status 301
     :headers {:location "/hop3" :content-length "0"}
     :body ""}

    # /hop3 → /end (307, preserves method/body)
    "/hop3"
    {:status 307
     :headers {:location "/end" :content-length "0"}
     :body ""}

    # /end: terminal
    "/end"
    (http:respond 200 (string req:method " /end"))

    # /loop → /loop (detect loop)
    "/loop"
    {:status 302
     :headers {:location "/loop" :content-length "0"}
     :body ""}

    # /abs → absolute URL redirect to /end
    "/abs"
    {:status 302
     :headers {:location (string "http://127.0.0.1:" redir-port "/end")
               :content-length "0"}
     :body ""}

    (http:respond 404 "not found")))

(def redir-fiber
  (ev/spawn (fn [] (http:serve redir-listener redir-handler))))

# No follow: redirect status is returned to caller
(let [[resp (http:get (string "http://127.0.0.1:" redir-port "/hop1"))]]
  (assert (= resp:status 302) "redirect: without :follow-redirects, returns 302"))

# :follow-redirects true: follows all hops to 200
(put redir-count 0 0)
(let [[resp (http:get (string "http://127.0.0.1:" redir-port "/hop1")
              :follow-redirects true)]]
  (assert (= resp:status 200) "redirect: :follow-redirects true reaches 200")
  (assert (= resp:body "GET /end") "redirect: method is GET after rewrite")
  (assert (= (get redir-count 0) 4)
    "redirect: server saw 4 requests (hop1, hop2, hop3, end)"))

# :follow-redirects integer: limits hops
(let [[resp (http:get (string "http://127.0.0.1:" redir-port "/hop1")
              :follow-redirects 1)]]
  (assert (= resp:status 301)
    "redirect: hop limit 1 stops at the second redirect response"))

# 307 preserves method on POST
(let [[resp (http:post (string "http://127.0.0.1:" redir-port "/hop3")
              "payload"
              :follow-redirects true)]]
  (assert (= resp:status 200) "redirect: 307 POST reaches end")
  (assert (= resp:body "POST /end") "redirect: 307 preserves POST method"))

# Loop detection: we bound hops, so a redirect loop terminates with
# the last redirect response rather than hanging.
(let [[resp (http:get (string "http://127.0.0.1:" redir-port "/loop")
              :follow-redirects 3)]]
  (assert (= resp:status 302) "redirect: loops stop at hop limit")
  (assert (= (get resp:headers :location) "/loop")
    "redirect: loop final response has the Location header"))

# Absolute Location URL works
(let [[resp (http:get (string "http://127.0.0.1:" redir-port "/abs")
              :follow-redirects true)]]
  (assert (= resp:status 200) "redirect: absolute Location URL followed")
  (assert (= resp:body "GET /end") "redirect: absolute Location produced GET"))

(port/close redir-listener)

# ============================================================================
# Server-Sent Events: server emits, client coroutine consumes
# ============================================================================

(def sse-listener (tcp/listen "127.0.0.1" 0))
(def sse-server-port
  (let [[parts (string/split (port/path sse-listener) ":")]]
    (parse-int (get parts (- (length parts) 1)))))

(defn sse-test-handler [req]
  (case req:path
    "/stream"
    (http:sse-response
      (fn [send]
        (send {:data "first"})
        (send {:event "tick" :data "1" :id "a"})
        (send {:event "tick" :data "2" :id "b"})
        (send {:data "multi\nline"})))

    "/retry-then-done"
    (http:sse-response
      (fn [send]
        (send {:retry 50 :id "x"})
        (send {:event "once" :data "hello" :id "x"})))

    # /echo-post: POST-only handler that streams back a synthetic
    # chat-completion-style response, echoing the request body in the
    # first event so the test can verify end-to-end delivery.
    "/echo-post"
    (if (= req:method "POST")
      (http:sse-response
        (fn [send]
          (send {:event "meta" :data (string "ct=" (or (get req:headers :content-type) "none"))})
          (send {:event "delta" :data (string "body=" (or req:body ""))})
          (send {:event "delta" :data "token-1"})
          (send {:event "delta" :data "token-2"})
          (send {:event "done"  :data "[DONE]"})))
      (http:respond 405 "method not allowed"))

    # /bad-post: POST handler that rejects with 400 so we can verify
    # sse-post's error surface.
    "/bad-post"
    (http:respond 400 "nope")

    (http:respond 404 "not found")))

(def sse-fiber
  (ev/spawn (fn [] (http:serve sse-listener sse-test-handler))))

# Basic stream: connect, consume all events, stop when stream ends.
(let [[events @[]]
      [source (http:sse-get
                (string "http://127.0.0.1:" sse-server-port "/stream")
                :reconnect false)]]
  (each evt in source
    (push events evt))
  (assert (= (length events) 4) "sse: received all four events")
  (let [[[e0 e1 e2 e3] events]]
    (assert (= e0:event "message") "sse: first event defaults to 'message'")
    (assert (= e0:data  "first")   "sse: first event data")
    (assert (= e1:event "tick")    "sse: named event type")
    (assert (= e1:id    "a")       "sse: id captured")
    (assert (= e2:id    "b")       "sse: id advances")
    (assert (= e3:data  "multi\nline")
      "sse: multi-line data preserved as single payload")))

# ============================================================================
# sse-post: POST with body, consume streamed SSE response
# ============================================================================

# Happy path: POST a JSON body, expect events echoing the body.
(let [[events @[]]
      [source (http:sse-post
                (string "http://127.0.0.1:" sse-server-port "/echo-post")
                "{\"prompt\":\"hi\"}")]]
  (each evt in source
    (push events evt))
  (assert (= (length events) 5) "sse-post: received all five events")
  (let [[[meta body t1 t2 done] events]]
    (assert (= meta:event "meta")               "sse-post: first event name")
    (assert (string/contains? meta:data "json")
      "sse-post: server saw Content-Type application/json by default")
    (assert (= body:event "delta")              "sse-post: body echo event")
    (assert (= body:data  "body={\"prompt\":\"hi\"}")
      "sse-post: request body delivered to server intact")
    (assert (= t1:data  "token-1")              "sse-post: token-1")
    (assert (= t2:data  "token-2")              "sse-post: token-2")
    (assert (= done:event "done")               "sse-post: terminal event")
    (assert (= done:data  "[DONE]")
      "sse-post: caller can detect OpenAI-style [DONE] sentinel")))

# Custom headers on sse-post are merged into the request.
(let [[events @[]]
      [source (http:sse-post
                (string "http://127.0.0.1:" sse-server-port "/echo-post")
                "raw bytes"
                :headers {:content-type "text/plain"})]]
  (each evt in source
    (push events evt))
  (let [[meta (first events)]]
    (assert (string/contains? meta:data "text/plain")
      "sse-post: :headers override the default Content-Type")))

# Error path: non-2xx response signals :sse-bad-status (coroutine body
# errors during draining; the error surfaces to the each-loop driver).
(let [[[ok? err] (protect
                   (let [[source (http:sse-post
                                   (string "http://127.0.0.1:" sse-server-port "/bad-post")
                                   "ignored")]]
                     (each _ in source nil)))]]
  (assert (not ok?)                         "sse-post: non-2xx signals error")
  (assert (= err:reason :sse-bad-status)    "sse-post: reason is :sse-bad-status")
  (assert (= err:status 400)                "sse-post: reports server status"))

(port/close sse-listener)

(println "all http tests passed")
