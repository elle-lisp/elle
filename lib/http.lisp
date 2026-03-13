## lib/http.lisp — Pure Elle HTTP/1.1 client and server
##
## Loaded via: (def http ((import-file "./lib/http.lisp")))
## Usage:      (http:get "http://example.com/")

# ============================================================================
# Internal helpers
# ============================================================================

(defn merge-structs [base override]
  "Merge two immutable structs. Keys in override win."
  (let ((result (@struct)))
    (each k in (keys base)
      (put result k (get base k)))
    (each k in (keys override)
      (put result k (get override k)))
    result))

# ============================================================================
# Chunk 1: URL parsing
# ============================================================================

(defn parse-url [url]
  "Parse an HTTP URL string into {:scheme :host :port :path :query}.
   Only 'http' scheme is supported. Default port is 80. Default path is '/'.
   Query is nil if absent, otherwise the string after '?' without the '?'."
  (when (not (string-starts-with? url "http://"))
    (error {:error :http-error :message (string/format "parse-url: unsupported scheme in: {}" url)}))
  (let ((rest (slice url 7 (length url))))   # strip "http://"
    (when (= (length rest) 0)
      (error {:error :http-error :message "parse-url: missing host"}))
    # Split host+port from path. Find first '/'.
    (let ((slash-pos (string/find rest "/")))
      (let ((authority (if (nil? slash-pos)
                          rest
                          (slice rest 0 slash-pos)))
            (path-and-query (if (nil? slash-pos)
                               "/"
                               (slice rest slash-pos (length rest)))))
        # Split authority into host and optional port.
        (let ((colon-pos (string/find authority ":")))
          (let ((host (if (nil? colon-pos)
                         authority
                         (slice authority 0 colon-pos)))
                (port (if (nil? colon-pos)
                         80
                         (integer (slice authority (+ colon-pos 1) (length authority))))))
            (when (= (length host) 0)
              (error {:error :http-error :message "parse-url: empty host"}))
            # Split path from query string.
            (let ((q-pos (string/find path-and-query "?")))
              (let ((path (if (nil? q-pos)
                             path-and-query
                             (slice path-and-query 0 q-pos)))
                    (query (if (nil? q-pos)
                              nil
                              (slice path-and-query (+ q-pos 1) (length path-and-query)))))
                {:scheme "http"
                 :host   host
                 :port   port
                 :path   path
                 :query  query}))))))))

# ============================================================================
# Chunk 2: Header parsing and serialization
# ============================================================================

(defn header-name->keyword [name]
  "Convert HTTP header name string to lowercase keyword.
   'Content-Type' -> :content-type"
  (keyword (string/downcase name)))

(defn keyword->header-name [kw]
  "Convert keyword to HTTP header name with capitalized segments.
   :content-type -> 'Content-Type'"
  (let ((parts (string/split (string kw) "-")))
    (string/join
      (map (fn [part]
             (if (= (length part) 0)
                 part
                 (string/format "{}{}"
                   (string/upcase (slice part 0 1))
                   (slice part 1 (length part)))))
           parts)
      "-")))

(defn read-headers [port]
  "Read HTTP headers from port until blank line. Returns immutable struct.
   Header keys are lowercase keywords (:content-type, :host, etc.).
   Stops when it reads an empty line (CRLF-only or LF-only).
   Signals :http-error on malformed header lines."
  (let ((headers (@struct)))
    (forever
      (let ((line (stream/read-line port)))
        (when (= (length line) 0)
          (break (freeze headers)))
        (let ((colon-pos (string/find line ":")))
          (when (nil? colon-pos)
            (error {:error :http-error
                    :message (string/format "read-headers: malformed header: {}" line)}))
          (let ((name (slice line 0 colon-pos))
                (value (string/trim (slice line (+ colon-pos 1) (length line)))))
            (put headers (header-name->keyword name) value)))))))

(defn write-headers [port headers]
  "Write HTTP headers struct to port. Each header is written as 'Name: value\\r\\n'.
   Keys are keywords converted back to HTTP header name casing."
  (each k in (keys headers)
    (stream/write port
      (string/format "{}: {}\r\n"
        (keyword->header-name k)
        (get headers k)))))

# ============================================================================
# Chunk 3: Request and response wire format
# ============================================================================

(defn read-request-line [port]
  "Read and parse HTTP request line: 'GET /path HTTP/1.1'.
   Returns {:method :path :version} (all strings).
   Signals :http-error on malformed input."
  (let ((line (stream/read-line port)))
    (let ((parts (string/split line " ")))
      (when (< (length parts) 3)
        (error {:error :http-error
                :message (string/format "read-request-line: malformed: {}" line)}))
      {:method  (get parts 0)
       :path    (get parts 1)
       :version (get parts 2)})))

(defn write-request-line [port method path]
  "Write HTTP request line: 'METHOD path HTTP/1.1\\r\\n'."
  (stream/write port (string/format "{} {} HTTP/1.1\r\n" method path)))

(defn read-status-line [port]
  "Read and parse HTTP status line: 'HTTP/1.1 200 OK'.
   Returns {:version :status :reason} where :status is an integer.
   Signals :http-error on malformed input."
  (let ((line (stream/read-line port)))
    (let ((parts (string/split line " ")))
      (when (< (length parts) 2)
        (error {:error :http-error
                :message (string/format "read-status-line: malformed: {}" line)}))
      (let ((version (get parts 0))
            (status  (integer (get parts 1)))
            # Reason phrase may be multi-word or absent; join remaining parts.
            (reason  (if (> (length parts) 2)
                         (string/join (slice parts 2 (length parts)) " ")
                         "")))
        {:version version :status status :reason reason}))))

(defn write-status-line [port status reason]
  "Write HTTP status line: 'HTTP/1.1 status reason\\r\\n'."
  (stream/write port (string/format "HTTP/1.1 {} {}\r\n" status reason)))

(defn read-body [port headers]
  "Read request/response body based on Content-Length header.
   Returns body string, or nil if Content-Length is absent."
  (let ((cl (get headers :content-length)))
    (if (nil? cl)
        nil
        (stream/read port (integer cl)))))

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

(defn http-request [method url &keys {:body body :headers extra-headers}]
  "Make an HTTP/1.1 request. Returns {:status :headers :body}.
   method: string (\"GET\", \"POST\", etc.)
   url: string parsed by parse-url
   :body optional string body
   :headers optional struct of extra headers to send
   # NOTE: Content-Length computed via (length body) — correct for ASCII,
   # wrong for non-ASCII. v1 limitation."
  (let ((parsed (parse-url url)))
    (let ((host (get parsed :host))
          (port-num (get parsed :port))
          (path (get parsed :path))
          (query (get parsed :query)))
      (let ((request-path (if (nil? query) path
                             (string/format "{}?{}" path query))))
        (let ((conn (tcp/connect host port-num)))
          (defer (port/close conn)
            # Write request line
            (write-request-line conn method request-path)
            # Write headers
            (let ((base-headers {:host host
                                  :connection "close"}))
              (let ((all-headers (if (nil? extra-headers)
                                     base-headers
                                     (merge-structs base-headers extra-headers))))
                # Add Content-Length if body present
                (let ((send-headers (if (nil? body)
                                        all-headers
                                        (merge-structs all-headers
                                          {:content-length (string (length body))}))))
                  (write-headers conn send-headers)
                  (stream/write conn "\r\n")
                  # Write body if present
                  (when (not (nil? body))
                    (stream/write conn body))
                  (stream/flush conn)
                  # Read response
                  (let ((status-line (read-status-line conn)))
                    (let ((resp-headers (read-headers conn)))
                      (let ((resp-body (read-body conn resp-headers)))
                        {:status  (get status-line :status)
                         :headers resp-headers
                         :body    resp-body}))))))))))))

(defn http-get [url &keys {:headers headers}]
  "Make a GET request. Returns {:status :headers :body}."
  (http-request "GET" url :headers headers))

(defn http-post [url body &keys {:headers headers}]
  "Make a POST request with body. Returns {:status :headers :body}."
  (http-request "POST" url :body body :headers headers))

# ============================================================================
# Module export closure
# ============================================================================

(fn []
  {:parse-url            parse-url
   :header-name->keyword header-name->keyword
   :keyword->header-name keyword->header-name
   :read-headers         read-headers
   :write-headers        write-headers
   :read-request-line    read-request-line
   :write-request-line   write-request-line
   :read-status-line     read-status-line
   :write-status-line    write-status-line
   :read-body            read-body
   :reason-phrases       reason-phrases
   :http-request         http-request
   :http-get             http-get
   :http-post            http-post})
