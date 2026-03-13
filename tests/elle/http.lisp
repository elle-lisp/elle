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
