# HTTP module tests

(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false
      :assert-err assert-err :assert-err-kind assert-err-kind
      :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq}
  ((import-file "tests/elle/assert.lisp")))

(def http ((import-file "lib/http.lisp")))
(def parse-url (get http :parse-url))

# ============================================================================
# Chunk 1: URL parsing
# ============================================================================

# Full URL with all components
(let ((u (parse-url "http://example.com:8080/api/users?page=2")))
  (assert-eq (get u :scheme) "http"         "full url: scheme")
  (assert-eq (get u :host)   "example.com"  "full url: host")
  (assert-eq (get u :port)   8080           "full url: port")
  (assert-eq (get u :path)   "/api/users"   "full url: path")
  (assert-eq (get u :query)  "page=2"        "full url: query"))

# Default port (80)
(let ((u (parse-url "http://example.com/index.html")))
  (assert-eq (get u :port) 80 "default port is 80")
  (assert-eq (get u :path) "/index.html" "path with default port"))

# No path defaults to "/"
(let ((u (parse-url "http://example.com")))
  (assert-eq (get u :path) "/" "no path defaults to /")
  (assert-true (nil? (get u :query)) "no query is nil"))

# Query string present, no path
(let ((u (parse-url "http://example.com/?q=hello")))
  (assert-eq (get u :path)  "/"       "path is / with query")
  (assert-eq (get u :query) "q=hello" "query parsed"))

# Trailing slash, no query
(let ((u (parse-url "http://localhost:3000/")))
  (assert-eq (get u :host) "localhost" "localhost host")
  (assert-eq (get u :port) 3000        "port 3000")
  (assert-eq (get u :path) "/"         "path /")
  (assert-true (nil? (get u :query))   "no query"))

# Error: non-http scheme
(assert-err-kind
  (fn () (parse-url "ftp://example.com/"))
  :http-error
  "ftp scheme signals :http-error")

# Error: malformed (no scheme)
(assert-err-kind
  (fn () (parse-url "example.com/foo"))
  :http-error
  "bare hostname signals :http-error")

# Error: https not supported
(assert-err-kind
  (fn () (parse-url "https://example.com/"))
  :http-error
  "https signals :http-error")

# ============================================================================
# Chunk 2: Header parsing and serialization
# ============================================================================

(def header-name->keyword (get http :header-name->keyword))
(def keyword->header-name (get http :keyword->header-name))
(def read-headers  (get http :read-headers))
(def write-headers (get http :write-headers))

# header-name->keyword
(assert-eq (header-name->keyword "Content-Type") :content-type
  "Content-Type -> :content-type")
(assert-eq (header-name->keyword "Host") :host
  "Host -> :host")
(assert-eq (header-name->keyword "X-Custom-Header") :x-custom-header
  "X-Custom-Header -> :x-custom-header")
(assert-eq (header-name->keyword "content-type") :content-type
  "already lowercase: content-type -> :content-type")

# keyword->header-name
(assert-eq (keyword->header-name :content-type) "Content-Type"
  ":content-type -> Content-Type")
(assert-eq (keyword->header-name :host) "Host"
  ":host -> Host")
(assert-eq (keyword->header-name :x-custom-header) "X-Custom-Header"
  ":x-custom-header -> X-Custom-Header")
(assert-eq (keyword->header-name :content-length) "Content-Length"
  ":content-length -> Content-Length")

# Round-trip: header-name->keyword -> keyword->header-name
(assert-eq (keyword->header-name (header-name->keyword "Content-Type")) "Content-Type"
  "round-trip Content-Type")
(assert-eq (keyword->header-name (header-name->keyword "Host")) "Host"
  "round-trip Host")

# read-headers: parse from a file used as port
(spit "/tmp/elle-http-test-headers"
  "Content-Type: text/plain\r\nHost: example.com\r\nContent-Length: 42\r\n\r\n")
(let ((p (port/open "/tmp/elle-http-test-headers" :read)))
  (let ((h (ev/spawn (fn [] (read-headers p)))))
    (port/close p)
    (assert-eq (get h :content-type)   "text/plain"   "content-type header")
    (assert-eq (get h :host)           "example.com"  "host header")
    (assert-eq (get h :content-length) "42"            "content-length header")))

# read-headers: whitespace trimmed from values
(spit "/tmp/elle-http-test-headers-ws"
  "X-Foo:   bar baz   \r\n\r\n")
(let ((p (port/open "/tmp/elle-http-test-headers-ws" :read)))
  (let ((h (ev/spawn (fn [] (read-headers p)))))
    (port/close p)
    (assert-eq (get h :x-foo) "bar baz" "leading/trailing whitespace trimmed")))

# read-headers: malformed line signals :http-error
(spit "/tmp/elle-http-test-headers-bad" "no-colon-here\r\n\r\n")
(assert-err-kind
  (fn ()
    (let ((p (port/open "/tmp/elle-http-test-headers-bad" :read)))
      (let ((result (ev/spawn (fn [] (read-headers p)))))
        (port/close p)
        result)))
  :http-error
  "malformed header line signals :http-error")

# write-headers: verify format
(let ((p (port/open "/tmp/elle-http-test-write-headers" :write)))
  (ev/spawn (fn []
    (write-headers p {:content-type "text/plain" :content-length "11"})
    (stream/write p "\r\n")
    (stream/flush p)))
  (port/close p))
(let ((content (slurp "/tmp/elle-http-test-write-headers")))
  (assert-true (string-contains? content "Content-Type: text/plain")
    "write-headers: content-type line present")
  (assert-true (string-contains? content "Content-Length: 11")
    "write-headers: content-length line present"))

# ============================================================================
# Chunk 3: Request and response wire format
# ============================================================================

(def read-request-line  (get http :read-request-line))
(def write-request-line (get http :write-request-line))
(def read-status-line   (get http :read-status-line))
(def write-status-line  (get http :write-status-line))
(def read-body          (get http :read-body))

# --- read-request-line ---

# Valid GET request line
(spit "/tmp/elle-http-test-req-line" "GET /path HTTP/1.1\r\n")
(let ((p (port/open "/tmp/elle-http-test-req-line" :read)))
  (let ((rl (ev/spawn (fn [] (read-request-line p)))))
    (port/close p)
    (assert-eq (get rl :method)  "GET"      "request-line: method")
    (assert-eq (get rl :path)    "/path"    "request-line: path")
    (assert-eq (get rl :version) "HTTP/1.1" "request-line: version")))

# POST with query string in path
(spit "/tmp/elle-http-test-req-line-post" "POST /api/data?x=1 HTTP/1.1\r\n")
(let ((p (port/open "/tmp/elle-http-test-req-line-post" :read)))
  (let ((rl (ev/spawn (fn [] (read-request-line p)))))
    (port/close p)
    (assert-eq (get rl :method) "POST"           "request-line: POST method")
    (assert-eq (get rl :path)   "/api/data?x=1"  "request-line: path with query")))

# Malformed request line (no spaces)
(spit "/tmp/elle-http-test-req-line-bad" "MALFORMED\r\n")
(assert-err-kind
  (fn ()
    (let ((p (port/open "/tmp/elle-http-test-req-line-bad" :read)))
      (let ((result (ev/spawn (fn [] (read-request-line p)))))
        (port/close p)
        result)))
  :http-error
  "malformed request line signals :http-error")

# --- read-status-line ---

# Standard 200 OK
(spit "/tmp/elle-http-test-status-200" "HTTP/1.1 200 OK\r\n")
(let ((p (port/open "/tmp/elle-http-test-status-200" :read)))
  (let ((sl (ev/spawn (fn [] (read-status-line p)))))
    (port/close p)
    (assert-eq (get sl :version) "HTTP/1.1" "status-line: version")
    (assert-eq (get sl :status)  200         "status-line: status integer")
    (assert-eq (get sl :reason)  "OK"        "status-line: reason")))

# Multi-word reason phrase
(spit "/tmp/elle-http-test-status-404" "HTTP/1.1 404 Not Found\r\n")
(let ((p (port/open "/tmp/elle-http-test-status-404" :read)))
  (let ((sl (ev/spawn (fn [] (read-status-line p)))))
    (port/close p)
    (assert-eq (get sl :status) 404         "multi-word reason: status")
    (assert-eq (get sl :reason) "Not Found" "multi-word reason: reason")))

# Status 500 Internal Server Error
(spit "/tmp/elle-http-test-status-500" "HTTP/1.1 500 Internal Server Error\r\n")
(let ((p (port/open "/tmp/elle-http-test-status-500" :read)))
  (let ((sl (ev/spawn (fn [] (read-status-line p)))))
    (port/close p)
    (assert-eq (get sl :status) 500                     "500 status")
    (assert-eq (get sl :reason) "Internal Server Error"  "500 reason")))

# Malformed status line
(spit "/tmp/elle-http-test-status-bad" "NOTHTTP\r\n")
(assert-err-kind
  (fn ()
    (let ((p (port/open "/tmp/elle-http-test-status-bad" :read)))
      (let ((result (ev/spawn (fn [] (read-status-line p)))))
        (port/close p)
        result)))
  :http-error
  "malformed status line signals :http-error")

# --- read-body ---

# With Content-Length
(spit "/tmp/elle-http-test-body" "hello world")
(let ((p (port/open "/tmp/elle-http-test-body" :read)))
  (let ((body (ev/spawn (fn [] (read-body p {:content-length "11"})))))
    (port/close p)
    (assert-eq body "hello world" "read-body with content-length")))

# Without Content-Length -> nil
(spit "/tmp/elle-http-test-body-no-cl" "ignored content")
(let ((p (port/open "/tmp/elle-http-test-body-no-cl" :read)))
  (let ((body (ev/spawn (fn [] (read-body p {})))))
    (port/close p)
    (assert-true (nil? body) "read-body without content-length is nil")))

# Full request parse: request line + headers + body via file port.
# All reads in a single ev/spawn fiber to share port position.
# Use a mutable array defined outside the fiber to capture values:
# strings from I/O become invalid after the next I/O yield inside
# the fiber, but writes to an outer @array survive.
(spit "/tmp/elle-http-test-full-req"
  "POST /submit HTTP/1.1\r\nHost: localhost\r\nContent-Length: 4\r\n\r\ndata")
(let ((out @[nil nil nil nil]))
  (let ((p (port/open "/tmp/elle-http-test-full-req" :read)))
    (ev/spawn (fn []
      (let ((rl (read-request-line p)))
        (put out 0 (get rl :method))
        (put out 1 (get rl :path))
        (let ((h (read-headers p)))
          (put out 2 (get h :host))
          (let ((body (read-body p h)))
            (put out 3 body))))))
    (port/close p))
  (assert-eq (get out 0) "POST"      "full req: method")
  (assert-eq (get out 1) "/submit"   "full req: path")
  (assert-eq (get out 2) "localhost" "full req: host header")
  (assert-eq (get out 3) "data"      "full req: body"))

# ============================================================================
# Chunk 4: Client API — error paths
# ============================================================================

(def http-request (get http :http-request))
(def http-get     (get http :http-get))
(def http-post    (get http :http-post))

# Connection refused (nothing listening on port 1)
(assert-err
  (fn () (ev/spawn (fn () (http-get "http://127.0.0.1:1/"))))
  "http-get: connection refused signals error")
