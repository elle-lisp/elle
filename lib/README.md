# lib/ — Reusable Elle modules

| Module | Purpose | Docs |
|--------|---------|------|
| `http.lisp` | HTTP/1.1 client and server over TCP | [below](#http) |
| `tls.lisp` | TLS 1.2/1.3 client and server | |
| `redis.lisp` | Redis client (RESP2) over TCP | |
| `dns.lisp` | DNS client (RFC 1035) | |
| `aws.lisp` | AWS client: SigV4 signing, HTTPS requests | [`aws/README.md`](aws/README.md) |
| `aws/` | Generated AWS service modules + SigV4 | [`aws/README.md`](aws/README.md) |
| `contract.lisp` | Compositional validation for function boundaries | |
| `lua.lisp` | Lua standard library compatibility prelude | |

Each module is a closure. `import-file` loads it; calling the result
initializes the module and returns a struct of its exports. Modules
that depend on other modules or plugins take them as arguments.

```lisp
(def http ((import-file "lib/http.lisp")))
(def tls  ((import-file "lib/tls.lisp") tls-plugin))
(def aws  ((import-file "lib/aws.lisp") crypto jiff tls))
```

---

<a id="http"></a>
# HTTP

Pure Elle HTTP/1.1 client and server.

## Quick start

**Server:**
```lisp
(def http ((import-file "./lib/http.lisp")))

(http:serve 8080
  (fn [request]
    (http:respond 200 (string/format "Hello from {}" (get request :path)))))
```

**Client:**
```lisp
(def http ((import-file "./lib/http.lisp")))

(def resp (ev/spawn (fn () (http:get "http://example.com/"))))
(println (get resp :status))  # 200
(println (get resp :body))
```

## API reference

### `(http:get url &keys {:headers})`

Make a GET request. Returns `{:status :headers :body}`.

**Parameters:**
- `url` (string): HTTP URL to request
- `:headers` (optional struct): Additional headers to send

**Returns:** Response struct with `:status` (integer), `:headers` (struct), `:body` (string or nil)

**Example:**
```lisp
(let ((resp (http:get "http://example.com/")))
  (println (get resp :status))
  (println (get resp :body)))
```

### `(http:post url body &keys {:headers})`

Make a POST request. `body` is a string. Returns `{:status :headers :body}`.

**Parameters:**
- `url` (string): HTTP URL to request
- `body` (string): Request body
- `:headers` (optional struct): Additional headers to send

**Returns:** Response struct

**Example:**
```lisp
(let ((resp (http:post "http://example.com/api" "{\"key\": \"value\"}")))
  (println (get resp :status)))
```

### `(http:request method url &keys {:body :headers})`

General request. `method` is a string (`"GET"`, `"POST"`, `"PUT"`, etc.).

**Parameters:**
- `method` (string): HTTP method
- `url` (string): HTTP URL to request
- `:body` (optional string): Request body
- `:headers` (optional struct): Additional headers to send

**Returns:** Response struct

### `(http:serve port-num handler)`

Start a server on `port-num`. `handler` is `(fn [request]) → response`.
Runs the accept loop with `ev/run`. Runs until the process exits or the
listener is closed.

**Parameters:**
- `port-num` (integer): Port to listen on (0 = OS-assigned)
- `handler` (function): `(fn [request]) → response` where request is `{:method :path :version :headers :body}`

**Returns:** nil (runs forever)

**Example:**
```lisp
(http:serve 8080
  (fn [request]
    (if (= (get request :method) "GET")
        (http:respond 200 "GET response")
        (http:respond 405 "Method not allowed"))))
```

### `(http:respond status body &keys {:headers})`

Build a response struct. Sets `Content-Type: text/plain` and `Content-Length`
automatically. Override with `:headers`.

**Parameters:**
- `status` (integer): HTTP status code
- `body` (string): Response body
- `:headers` (optional struct): Override headers

**Returns:** Response struct `{:status :headers :body}`

**Example:**
```lisp
(http:respond 200 "Hello"
  :headers {:content-type "text/html"})
```

### `(http:parse-url url)`

Parse a URL string into `{:scheme :host :port :path :query}`.

**Parameters:**
- `url` (string): HTTP URL to parse

**Returns:** URL struct with `:scheme`, `:host`, `:port`, `:path`, `:query`

**Example:**
```lisp
(let ((u (http:parse-url "http://example.com:8080/api?q=test")))
  (println (get u :host))   # "example.com"
  (println (get u :port))   # 8080
  (println (get u :path))   # "/api"
  (println (get u :query))) # "q=test"
```

## Limitations

- HTTP only (no HTTPS/TLS)
- Content-Length bodies only (no chunked transfer encoding)
- No connection pooling or keep-alive
- No redirect following
- Content-Length incorrect for non-ASCII bodies (counted by grapheme clusters)
- HTTP/2 and HTTP/3 not supported

## Loading

```lisp
(def http ((import-file "./lib/http.lisp")))
# Use as http:get, http:post, etc.
```

## Error handling

All HTTP errors signal with `:http-error` kind:

```lisp
(try
  (http:get "http://invalid-host/")
  (catch [err]
    (println (get err :error))    # :http-error
    (println (get err :message))))
```

## Concurrency

The server uses `ev/run` for concurrent connection handling. Each accepted
connection runs in its own fiber. The client uses `tcp/connect` which yields
(SIG_IO) and must run inside a scheduler context.

```lisp
# Client must run inside ev/run or ev/spawn
(ev/run
  (fn []
    (let ((resp (http:get "http://example.com/")))
      (println (get resp :status)))))
```
