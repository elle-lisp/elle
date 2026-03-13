## lib/http.lisp — Pure Elle HTTP/1.1 client and server
##
## Loaded via: (def http ((import-file "./lib/http.lisp")))
## Usage:      (http:get "http://example.com/")

# ============================================================================
# Chunk 1: URL parsing
# ============================================================================

(defn parse-url [url]
  "Parse an HTTP URL string into {:scheme :host :port :path :query}.
   Only 'http' scheme is supported. Default port is 80. Default path is '/'.
   Query is nil if absent, otherwise the string after '?' without the '?'."
  (when (not (string-starts-with? url "http://"))
    (error {:error :http-error :message "unsupported scheme" :url url}))
  (let* [[rest (slice url 7 (length url))]
         [slash-pos (string/find rest "/")]
         [authority (if (nil? slash-pos) rest (slice rest 0 slash-pos))]
         [path-and-query (if (nil? slash-pos) "/" (slice rest slash-pos (length rest)))]
         [colon-pos (string/find authority ":")]
         [host (if (nil? colon-pos) authority (slice authority 0 colon-pos))]
         [port (if (nil? colon-pos) 80 (integer (slice authority (+ colon-pos 1) (length authority))))]]
    (when (= (length rest) 0)
      (error {:error :http-error :message "missing host"}))
    (when (= (length host) 0)
      (error {:error :http-error :message "empty host"}))
    (let* [[q-pos (string/find path-and-query "?")]
           [path (if (nil? q-pos) path-and-query (slice path-and-query 0 q-pos))]
           [query (if (nil? q-pos) nil (slice path-and-query (+ q-pos 1) (length path-and-query)))]]
      {:scheme "http" :host host :port port :path path :query query})))

# ============================================================================
# Chunk 2: Header parsing and serialization
# ============================================================================

(defn header-name->keyword [name]
  "Convert HTTP header name string to lowercase keyword.
   'Content-Type' -> :content-type"
  (keyword (string/downcase name)))

(defn capitalize-segment [part]
  "Capitalize first letter of a string segment."
  (if (= (length part) 0)
      part
      (string/format "{}{}"
        (string/upcase (slice part 0 1))
        (slice part 1 (length part)))))

(defn keyword->header-name [kw]
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
  (let [[headers (@struct)]]
    (forever
      (let [[line (stream/read-line port)]]
        (when (= (length line) 0) (break (freeze headers)))
        (let [[colon-pos (string/find line ":")]]
          (when (nil? colon-pos)
            (error {:error :http-error :message "malformed header"}))
          (let* [[name (slice line 0 colon-pos)]
                 [value (string/trim (slice line (+ colon-pos 1) (length line)))]]
            (put headers (header-name->keyword name) value)))))))

(defn write-headers [port headers]
  "Write HTTP headers struct to port. Each header is written as 'Name: value\\r\\n'.
   Keys are keywords converted back to HTTP header name casing."
  (each k in (keys headers)
    (stream/write port (string/format "{}: {}\r\n" (keyword->header-name k) (get headers k)))))

# ============================================================================
# Chunk 3: Request and response wire format
# ============================================================================

(defn read-request-line [port]
  "Read and parse HTTP request line: 'GET /path HTTP/1.1'.
   Returns {:method :path :version} (all strings).
   Signals :http-error on malformed input."
  (let* [[line (stream/read-line port)]
         [parts (string/split line " ")]]
    (when (< (length parts) 3)
      (error {:error :http-error :message "malformed request line"}))
    {:method (get parts 0) :path (get parts 1) :version (get parts 2)}))

(defn write-request-line [port method path]
  "Write HTTP request line: 'METHOD path HTTP/1.1\\r\\n'."
  (stream/write port (string/format "{} {} HTTP/1.1\r\n" method path)))

(defn read-status-line [port]
  "Read and parse HTTP status line: 'HTTP/1.1 200 OK'.
   Returns {:version :status :reason} where :status is an integer.
   Signals :http-error on malformed input."
  (let* [[line (stream/read-line port)]
         [parts (string/split line " ")]]
    (when (< (length parts) 2)
      (error {:error :http-error :message "malformed status line"}))
    (let* [[[version status-str & reason-parts] parts]
           [status (integer status-str)]
           [reason (if (empty? reason-parts) "" (string/join reason-parts " "))]]
      {:version version :status status :reason reason})))

(defn write-status-line [port status reason]
  "Write HTTP status line: 'HTTP/1.1 status reason\\r\\n'."
  (stream/write port (string/format "HTTP/1.1 {} {}\r\n" status reason)))

(defn read-body [port headers]
  "Read request/response body based on Content-Length header.
   Returns body string, or nil if Content-Length is absent."
  (let [[cl (get headers :content-length)]]
    (if (nil? cl) nil (stream/read port (integer cl)))))

# ============================================================================
# Chunk 4: Client API
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
   409 "Conflict"
   500 "Internal Server Error"
   502 "Bad Gateway"
   503 "Service Unavailable"})

(defn build-request-headers [host extra-headers body]
  "Build request headers with host, connection, and optional Content-Length."
  (let [[headers (merge {:host host :connection "close"} (or extra-headers {}))]]
    (if (nil? body)
        headers
        (merge headers {:content-length (string (string/size-of body))}))))

(defn http-request [method url &keys {:body body :headers extra-headers}]
  "Make an HTTP/1.1 request. Returns {:status :headers :body}.
   method: string (\"GET\", \"POST\", etc.)
   url: string
   :body optional string body
   :headers optional struct of extra headers to send"
  (let* [[url-parsed (parse-url url)]
         [request-path (if (nil? url-parsed:query)
                           url-parsed:path
                           (string/format "{}?{}" url-parsed:path url-parsed:query))]
         [conn (tcp/connect url-parsed:host url-parsed:port)]]
    (defer (port/close conn)
      (write-request-line conn method request-path)
      (let [[headers (build-request-headers url-parsed:host extra-headers body)]]
        (write-headers conn headers)
        (stream/write conn "\r\n")
        (when (not (nil? body)) (stream/write conn body))
        (stream/flush conn)
        (let* [[status-line (read-status-line conn)]
               [resp-headers (read-headers conn)]
               [resp-body (read-body conn resp-headers)]]
          {:status status-line:status :headers resp-headers :body resp-body})))))

(defn http-get [url &keys {:headers headers}]
  "Make a GET request. Returns {:status :headers :body}."
  (http-request "GET" url :headers headers))

(defn http-post [url body &keys {:headers headers}]
  "Make a POST request with body. Returns {:status :headers :body}."
  (http-request "POST" url :body body :headers headers))

# ============================================================================
# Chunk 5: Server API
# ============================================================================

(defn read-request [conn]
  "Read a complete HTTP request from a connection port.
   Returns {:method :path :version :headers :body}."
  (let* [[req-line (read-request-line conn)]
         [headers (read-headers conn)]
         [body (read-body conn headers)]]
    {:method req-line:method :path req-line:path :version req-line:version
     :headers headers :body body}))

(defn write-response [conn response]
  "Write a complete HTTP response to a connection port and flush.
   response is {:status :headers :body}."
  (write-status-line conn response:status (or (get reason-phrases response:status) "Unknown"))
  (write-headers conn response:headers)
  (stream/write conn "\r\n")
  (when (not (nil? response:body)) (stream/write conn response:body))
  (stream/flush conn))

(defn http-respond [status body &keys {:headers extra-headers}]
  "Build a response struct with Content-Type and Content-Length set.
   Caller can override headers via :headers."
  (let* [[base-headers {:content-type "text/plain"
                        :content-length (string (string/size-of body))}]
         [merged (if (nil? extra-headers) base-headers (merge base-headers extra-headers))]]
    {:status status :headers merged :body body}))

(defn handle-connection [conn handler]
  "Handle a single HTTP connection: read request, call handler, write response.
   Errors in handler return a 500 response. Connection is closed on exit."
  (defer (port/close conn)
    (let* [[request (read-request conn)]
           [[ok? val] (protect (handler request))]
           [response (if ok? val (http-respond 500 "Internal Server Error"))]]
      (write-response conn response))))

(defn accept-loop [listener handler]
  "Accept connections and spawn fibers to handle them."
  (forever
    (let [[conn (tcp/accept listener)]]
      (ev/spawn (fn [] (handle-connection conn handler))))))

(defn http-serve [port-num handler]
  "Start an HTTP server on port-num. Calls handler for each request.
   handler: (fn [request]) -> response
   Runs until killed. Each connection runs in its own fiber.
   Errors in handler return a 500 response."
  (let [[listener (tcp/listen "0.0.0.0" port-num)]]
    (ev/run (fn [] (accept-loop listener handler)))))

# ============================================================================
# Module export closure
# ============================================================================

(fn []
  {:parse-url            parse-url
   :header-name->keyword header-name->keyword
   :keyword->header-name keyword->header-name
   :http-request         http-request
   :http-get             http-get
   :http-post            http-post
   :http-respond         http-respond
   :http-serve           http-serve
   :read-request         read-request
   :write-response       write-response})
