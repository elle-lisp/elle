## lib/http.lisp — Pure Elle HTTP/1.1 client and server
##
## Loaded via: (def http ((import-file "./lib/http.lisp")))
## Usage:      (http:get "http://example.com/")

# ============================================================================
# URL parsing
# ============================================================================

(defn parse-url [url]
  "Parse an HTTP URL string into {:scheme :host :port :path :query}.
   Only 'http' scheme is supported. Default port is 80. Default path is '/'.
   Query is nil if absent, otherwise the string after '?' without the '?'."
  (unless (string-starts-with? url "http://")
    (error {:error :http-error :message "unsupported scheme" :url url}))
  (let* [[tail (slice url (length "http://"))]
         [slash (string/find tail "/")]
         [auth (if (nil? slash) tail (slice tail 0 slash))]
         [path+query (if (nil? slash) "/" (slice tail slash))]
         [colon (string/find auth ":")]
         [host (if (nil? colon) auth (slice auth 0 colon))]
         [port (if (nil? colon) 80 (integer (slice auth (inc colon))))]]
    (when (empty? tail)
      (error {:error :http-error :message "missing host" :url url}))
    (when (empty? host)
      (error {:error :http-error :message "empty host" :url url}))
    (let* [[q-pos (string/find path+query "?")]
           [path (if (nil? q-pos) path+query (slice path+query 0 q-pos))]
           [query (if (nil? q-pos) nil (slice path+query (inc q-pos)))]]
      {:scheme "http" :host host :port port :path path :query query})))

# ============================================================================
# Header parsing and serialization
# ============================================================================

(defn header->kw [name]
  "Convert HTTP header name string to lowercase keyword.
   'Content-Type' -> :content-type"
  (keyword (string/lowercase name)))

(defn capitalize-segment [part]
  "Capitalize first letter of a string segment."
  (if (empty? part)
      part
      (concat (string/uppercase (first part)) (rest part))))

(defn kw->header [kw]
  "Convert keyword to HTTP header name with capitalized segments.
   :content-type -> 'Content-Type'"
  (let* [[parts (string/split (string kw) "-")]
         [capitalized (map capitalize-segment parts)]]
    (string/join capitalized "-")))

(defn read-headers [port]
  "Read HTTP headers from port until blank line. Returns immutable struct.
   Header keys are lowercase keywords (:content-type, :host, etc.).
   Stops when it reads an empty line (CRLF-only or LF-only).
   Signals :http-error on malformed header lines."
  (def headers @{})
  (forever
    (let [[line (port/read-line port)]]
      (when (or (nil? line) (empty? line))
        (break (freeze headers)))
      (let [[colon-pos (string/find line ":")]]
        (when (nil? colon-pos)
          (error {:error :http-error :message "malformed header"
                  :line line}))
        (let* [[name (slice line 0 colon-pos)]
               [value (string/trim (slice line (inc colon-pos)))]]
          (put headers (header->kw name) value))))))

(defn write-headers [port headers]
  "Write HTTP headers struct to port. Each header is written as 'Name: value\\r\\n'.
   Keys are keywords converted back to HTTP header name casing."
  (each [key value] in (pairs headers)
    (port/write port (string/format "{}: {}\r\n" (kw->header key) value))))

# ============================================================================
# Request and response wire format
# ============================================================================

(defn read-request-line [port]
  "Read and parse HTTP request line: 'GET /path HTTP/1.1'.
   Returns {:method :path :version} or nil on EOF."
  (let [[line (port/read-line port)]]
    (if (nil? line)
      nil
      (let [[parts (string/split line " ")]]
        (when (< (length parts) 3)
          (error {:error :http-error :message "malformed request line"
                  :line line}))
        (let [[[method path version] parts]]
          (unless (string/starts-with? version "HTTP/")
            (error {:error :http-error :message "invalid HTTP version"
                    :version version}))
          {:method method :path path :version version})))))

(defn write-request-line [port method path]
  "Write HTTP request line: 'METHOD path HTTP/1.1\\r\\n'."
  (port/write port (string/format "{} {} HTTP/1.1\r\n" method path)))

(defn read-status-line [port]
  "Read and parse HTTP status line: 'HTTP/1.1 200 OK'.
   Returns {:version :status :reason} where :status is an integer.
   Signals :http-error on malformed input."
  (let* [[line (port/read-line port)]
         [parts (string/split line " ")]]
    (when (< (length parts) 2)
      (error {:error :http-error :message "malformed status line"
              :line line}))
    (let* [[[version status-str & reason-parts] parts]
           [status (integer status-str)]
           [reason (if (empty? reason-parts) "" (string/join reason-parts " "))]]
      {:version version :status status :reason reason})))

(defn write-status-line [port status reason]
  "Write HTTP status line: 'HTTP/1.1 status reason\\r\\n'."
  (port/write port (string/format "HTTP/1.1 {} {}\r\n" status reason)))

(defn read-body [port headers]
  "Read request/response body based on Content-Length header.
   Returns body string, or nil if Content-Length is absent.
   Loops on short reads to handle large responses over TCP."
  (when headers:content-length
    (var n (integer headers:content-length))
    (if (= n 0)
      ""
      (begin
        (var buf (@bytes))
        (while (pos? n)
          (let [[chunk (port/read port n)]]
            (when (nil? chunk)
              (error {:error :http-error :message "unexpected EOF reading HTTP body"}))
            (let [[b (if (bytes? chunk) chunk (bytes chunk))]]
              (append buf b)
              (assign n (- n (length b))))))
        (string (freeze buf))))))

# ============================================================================
# Reason phrases
# ============================================================================

(def reason-phrases
  {200 "OK"
   201 "Created"
   204 "No Content"
   301 "Moved Permanently"
   302 "Found"
   304 "Not Modified"
   400 "Bad Request"
   401 "Unauthorized"
   403 "Forbidden"
   404 "Not Found"
   405 "Method Not Allowed"
   413 "Payload Too Large"
   409 "Conflict"
   500 "Internal Server Error"
   502 "Bad Gateway"
   503 "Service Unavailable"})

# ============================================================================
# Response construction
# ============================================================================

(defn http-respond [status body &named headers]
  "Build a response struct with Content-Type and Content-Length set.
   Caller can override headers via :headers."
  (let* [[base-headers {:content-type "text/plain"
                        :content-length (string (string/size-of body))}]
         [merged (if (nil? headers)
                   base-headers
                   (merge base-headers (freeze headers)))]]
    {:status status :headers merged :body body}))

# ============================================================================
# Client API
# ============================================================================

(defn wants-close? [headers]
  "True if headers indicate the connection should close."
  (let [[conn headers:connection]]
    (and conn (= (string/lowercase conn) "close"))))

(defn build-request-headers [host extra-headers body keep-alive]
  "Build request headers. Sets connection: keep-alive or close."
  (let [[headers (merge {:host host
                         :connection (if keep-alive "keep-alive" "close")}
                        (freeze (or extra-headers {})))]]
    (if (nil? body)
        headers
        (merge headers {:content-length (string (string/size-of body))}))))

(defn send-request [conn method path host extra-headers body keep-alive]
  "Send an HTTP request on an open connection. Returns response struct.
   Does NOT close the connection."
  (write-request-line conn method path)
  (let [[headers (build-request-headers host extra-headers body keep-alive)]]
    (write-headers conn headers)
    (port/write conn "\r\n")
    (unless (nil? body) (port/write conn body))
    (port/flush conn)
    (let* [[status-line (read-status-line conn)]
           [resp-headers (read-headers conn)]
           [resp-body (read-body conn resp-headers)]]
      {:status status-line:status :headers resp-headers :body resp-body})))

(defn http-request [method url &named body headers]
  "Make an HTTP/1.1 request. Opens a new connection, sends request, closes.
   Returns {:status :headers :body}."
  (let* [[url-parsed (parse-url url)]
         [request-path (if (nil? url-parsed:query)
                           url-parsed:path
                           (string/format "{}?{}" url-parsed:path url-parsed:query))]
         [conn (tcp/connect url-parsed:host url-parsed:port)]]
    (defer (port/close conn)
      (send-request conn method request-path
                    url-parsed:host headers body false))))

(defn http-get [url &named headers]
  "Make a GET request. Returns {:status :headers :body}."
  (http-request "GET" url :headers headers))

(defn http-post [url body &named headers]
  "Make a POST request with body. Returns {:status :headers :body}."
  (http-request "POST" url :body body :headers headers))

(defn http-connect [url]
  "Open a keep-alive connection to a URL's host:port.
   Returns {:conn :host :path-prefix} for use with http:send."
  (let* [[url-parsed (parse-url url)]
         [conn (tcp/connect url-parsed:host url-parsed:port)]]
    {:conn conn :host url-parsed:host}))

(defn http-send [session method path &named body headers]
  "Send a request on an existing keep-alive connection.
   session: struct from http:connect. Returns {:status :headers :body}.
   Connection remains open unless server sends connection: close."
  (send-request session:conn method path
                session:host headers body true))

(defn http-close [session]
  "Close a keep-alive session."
  (port/close session:conn))

# ============================================================================
# Server API
# ============================================================================

(defn read-request [conn]
  "Read a complete HTTP request from a connection port.
   Returns {:method :path :version :headers :body}, or nil on EOF."
  (when-let [[req-line (read-request-line conn)]]
    (let* [[headers (read-headers conn)]
           [body (read-body conn headers)]]
      {:method req-line:method
       :path req-line:path
       :version req-line:version
       :headers headers
       :body body})))

(defn write-response [conn response]
  "Write a complete HTTP response to a connection port and flush.
   response is {:status :headers :body}."
  (write-status-line conn response:status
                     (or (get reason-phrases response:status) "Unknown"))
  (write-headers conn response:headers)
  (port/write conn "\r\n")
  (unless (nil? response:body)
    (port/write conn response:body))
  (port/flush conn))

(defn connection-loop [conn handler on-error]
  "Handle HTTP requests on a connection until it closes or either side
   sends connection: close. Each request is read, passed to handler,
   and the response is written. Errors in handler produce a 500 response.
   on-error is called with (request error) when the handler fails."
  (defer (protect (port/close conn))
    (forever
      (let [[[ok? req] (protect (read-request conn))]]
        (unless ok? (break))
        (when (nil? req) (break))
        (let* [[[ok? val] (protect (handler req))]
               [response (if ok?
                           val
                           (begin
                             (when on-error (on-error req val))
                             (http-respond 500 "Internal Server Error")))]]
          (write-response conn response)
          (when (or (wants-close? req:headers)
                    (wants-close? response:headers))
            (break)))))))

(defn default-on-error [request err]
  "Default error handler: print to stderr."
  (port/write (port/stderr)
    (string/format "http: handler error on {} {}: {}\n"
                   request:method request:path err)))

(defn http-serve [listener handler &named on-error]
  "Accept connections on listener and handle them with keep-alive.
   Each connection runs in its own fiber via ev/spawn.
   Exits cleanly when the listener is closed.
   handler: (fn [request]) -> response
   :on-error: (fn [request error]) -> nil (default: print to stderr)"
  (default on-error default-on-error)
  (forever
    (let [[[ok? conn] (protect (tcp/accept listener))]]
      (unless ok? (break))
      (ev/spawn (fn [] (connection-loop conn handler on-error))))))

# ============================================================================
# Exports
# ============================================================================

(defn run-internal-tests []
  "Sanity checks on internal wire-format helpers. Called via (http:test)."

  # header->kw
  (assert (= (header->kw "Content-Type") :content-type) "header->kw Content-Type")
  (assert (= (header->kw "Host") :host)                 "header->kw Host")
  (assert (= (header->kw "X-Custom-Header") :x-custom-header)
    "header->kw X-Custom-Header")
  (assert (= (header->kw "content-type") :content-type) "header->kw lowercase")

  # kw->header
  (assert (= (kw->header :content-type) "Content-Type")     "kw->header content-type")
  (assert (= (kw->header :host) "Host")                     "kw->header host")
  (assert (= (kw->header :x-custom-header) "X-Custom-Header")
    "kw->header x-custom-header")
  (assert (= (kw->header :content-length) "Content-Length")  "kw->header content-length")

  # round-trip
  (assert (= (kw->header (header->kw "Content-Type")) "Content-Type")
    "header round-trip Content-Type")
  (assert (= (kw->header (header->kw "Host")) "Host")
    "header round-trip Host")

  # read-headers via file port
  (spit "/tmp/elle-http-test-headers"
    "Content-Type: text/plain\r\nHost: example.com\r\nContent-Length: 42\r\n\r\n")
  (let [[p (port/open "/tmp/elle-http-test-headers" :read)]]
    (let [[h (read-headers p)]]
      (port/close p)
      (assert (= h:content-type   "text/plain")  "read-headers content-type")
      (assert (= h:host           "example.com") "read-headers host")
      (assert (= h:content-length "42")           "read-headers content-length")))

  # read-headers trims whitespace
  (spit "/tmp/elle-http-test-headers-ws" "X-Foo:   bar baz   \r\n\r\n")
  (let [[p (port/open "/tmp/elle-http-test-headers-ws" :read)]]
    (let [[h (read-headers p)]]
      (port/close p)
      (assert (= h:x-foo "bar baz") "read-headers trims whitespace")))

  # read-headers malformed
  (spit "/tmp/elle-http-test-headers-bad" "no-colon-here\r\n\r\n")
  (let [[p-bad (port/open "/tmp/elle-http-test-headers-bad" :read)]]
    (let [[[ok? _] (protect (read-headers p-bad))]]
      (assert (not ok?) "read-headers malformed signals error")))

  # write-headers
  (let [[p (port/open "/tmp/elle-http-test-write-headers" :write)]]
    (write-headers p {:content-type "text/plain" :content-length "11"})
    (port/write p "\r\n")
    (port/flush p)
    (port/close p))
  (let [[content (slurp "/tmp/elle-http-test-write-headers")]]
    (assert (string-contains? content "Content-Type: text/plain")
      "write-headers content-type")
    (assert (string-contains? content "Content-Length: 11")
      "write-headers content-length"))

  # read-request-line
  (spit "/tmp/elle-http-test-req-line" "GET /path HTTP/1.1\r\n")
  (let [[p (port/open "/tmp/elle-http-test-req-line" :read)]]
    (let [[rl (read-request-line p)]]
      (port/close p)
      (assert (= rl:method  "GET")      "request-line method")
      (assert (= rl:path    "/path")    "request-line path")
      (assert (= rl:version "HTTP/1.1") "request-line version")))

  # read-status-line
  (spit "/tmp/elle-http-test-status-200" "HTTP/1.1 200 OK\r\n")
  (let [[p (port/open "/tmp/elle-http-test-status-200" :read)]]
    (let [[sl (read-status-line p)]]
      (port/close p)
      (assert (= sl:version "HTTP/1.1") "status-line version")
      (assert (= sl:status  200)         "status-line status")
      (assert (= sl:reason  "OK")        "status-line reason")))

  # read-body with Content-Length
  (spit "/tmp/elle-http-test-body" "hello world")
  (let [[p (port/open "/tmp/elle-http-test-body" :read)]]
    (let [[body (read-body p {:content-length "11"})]]
      (port/close p)
      (assert (= body "hello world") "read-body with content-length")))

  # read-body without Content-Length
  (spit "/tmp/elle-http-test-body-no-cl" "ignored")
  (let [[p (port/open "/tmp/elle-http-test-body-no-cl" :read)]]
    (let [[body (read-body p {})]]
      (port/close p)
      (assert (nil? body) "read-body without content-length is nil")))

  # full request parse
  (spit "/tmp/elle-http-test-full-req"
    "POST /submit HTTP/1.1\r\nHost: localhost\r\nContent-Length: 4\r\n\r\ndata")
  (let [[out @[nil nil nil nil]]]
    (let [[p (port/open "/tmp/elle-http-test-full-req" :read)]]
      (let [[rl (read-request-line p)]]
        (put out 0 rl:method)
        (put out 1 rl:path)
        (let [[h (read-headers p)]]
          (put out 2 h:host)
          (let [[body (read-body p h)]]
            (put out 3 body))))
      (port/close p))
    (assert (= (get out 0) "POST")      "full req method")
    (assert (= (get out 1) "/submit")   "full req path")
    (assert (= (get out 2) "localhost") "full req host")
    (assert (= (get out 3) "data")      "full req body"))

  true)

(fn []
  {:parse-url  parse-url

   # Response construction
   :respond    http-respond

   # Client — one-shot
   :get        http-get
   :post       http-post
   :request    http-request

   # Client — keep-alive
   :connect    http-connect
   :send       http-send
   :close      http-close

   # Server
   :serve      http-serve

   # Internal tests
   :test       run-internal-tests})
