# HTTP module tests
#
# Tests the public API of lib/http.lisp. Internal wire-format helpers
# are tested via (http:test) which runs sanity checks inside the module.

(def http ((import-file "lib/http.lisp")))

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

# Error: non-http scheme
(let [[[ok? err] (protect (http:parse-url "ftp://example.com/"))]]
  (assert (not ok?)                        "ftp scheme signals error")
  (assert (= (get err :error) :http-error) "ftp scheme is :http-error"))

# Error: malformed (no scheme)
(let [[[ok? err] (protect (http:parse-url "example.com/foo"))]]
  (assert (not ok?)                        "bare hostname signals error")
  (assert (= (get err :error) :http-error) "bare hostname is :http-error"))

# Error: https not supported
(let [[[ok? err] (protect (http:parse-url "https://example.com/"))]]
  (assert (not ok?)                        "https signals error")
  (assert (= (get err :error) :http-error) "https is :http-error"))

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
(let [[[ok? _] (protect (ev/spawn (fn [] (http:get "http://127.0.0.1:1/"))))]]
  (assert (not ok?) "http:get connection refused signals error"))

# ============================================================================
# Server + Client integration
# ============================================================================
# Deferred: requires ev/run cancellation so http:serve exits cleanly
# when the listener is closed. See pending.md Phase 2/3.

(println "all http tests passed")
