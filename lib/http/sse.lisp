(elle/epoch 13)
# audited: 2026-09-28
## Server-sent events: the client streams, the event parser, and the response builder.
## lib/http/overview.md
##
## Loaded via:
##   (def sse ((import "std/http/sse") :url url :transport transport :wire wire
##             :chunked chunked))
##
## Exports: {:get :post :response :format-event :test}

(fn [&named url transport wire chunked]
  (def parse-url url:parse-url)
  (def open-transport transport:open)
  (def t-read transport:read)
  (def t-read-line transport:read-line)
  (def t-write transport:write)
  (def t-flush transport:flush)
  (def t-close transport:close)
  (def write-request-line wire:write-request-line)
  (def write-headers wire:write-headers)
  (def build-request-headers wire:build-request-headers)
  (def read-status-line wire:read-status-line)
  (def read-headers wire:read-headers)
  (def read-body wire:read-body)
  (def chunked? chunked:chunked?)
  (def chunk-size chunked:chunk-size)

  (def sse-default-retry-ms 3000)

  (defn sse-strip-leading-space [s]
    "Per the SSE spec, a single leading space on a field value is
     eaten (so 'data: hello' yields data='hello')."
    (if (string/starts-with? s " ") (slice s 1) s))

  (defn sse-parse-field [line]
    "Parse one SSE field line per the HTML spec. Returns {:field :value},
     or nil for comments and unparseable input."
    (cond
      (empty? line) nil
      (string/starts-with? line ":") nil  # comment
      true
        (let [colon (string/find line ":")]
          (cond
            (nil? colon) {:field line :value ""}
            true
              {:field (slice line 0 colon)
               :value (sse-strip-leading-space (slice line (inc colon)))}))))

  (defn sse-for-each-body-line [t headers on-line]
    "Call (on-line line) for every body line. Handles both
     Transfer-Encoding: chunked and plain bodies. on-line receives lines
     with trailing CRLF/LF already stripped."
    (if (chunked? headers)
      (sse-for-each-body-line-chunked t on-line)
      (sse-for-each-body-line-plain t on-line)))

  (defn sse-for-each-body-line-plain [t on-line]
    (forever
      (let [line (t-read-line t)]
        (when (nil? line) (break))
        (on-line line))))

  (defn sse-drain-buffered-lines [buf on-line]
    "Yield all complete lines already present in buf. Returns the new
     buf value with the last (incomplete) line left behind."
    (def @remaining buf)
    (block :drain
      (forever
        (let [nl (string/find remaining "\n")]
          (when (nil? nl) (break :drain remaining))
          (let* [raw (slice remaining 0 nl)
                 line (if (string/ends-with? raw "\r")
                        (slice raw 0 (dec (length raw)))
                        raw)]
            (on-line line)
            (assign remaining (slice remaining (inc nl))))))))

  (defn sse-for-each-body-line-chunked [t on-line]
    (def @buf "")
    (block :chunks
      (forever
        (let [size (chunk-size (t-read-line t))]
          (when (= size 0)
            (forever
              (let [line (t-read-line t)]
                (when (or (nil? line) (empty? line)) (break))))
            (unless (empty? buf) (on-line buf))
            (break :chunks))
          (def @remaining size)
          (while (pos? remaining)
            (let [data (t-read t remaining)]
              (when (nil? data)
                (error {:error :http-error
                        :reason :unexpected-eof
                        :phase :chunk-data
                        :message "unexpected EOF reading chunk data"}))
              (let* [b (if (bytes? data) data (bytes data))
                     s (string b)]
                (assign buf (string buf s))
                (assign remaining (- remaining (length b))))))
          (t-read-line t)  # consume trailing CRLF after chunk data
          (assign buf (sse-drain-buffered-lines buf on-line))))))

  (defn sse-dispatch-event [state on-event]
    "If state has accumulated :data, emit an event via on-event and
     reset the per-event accumulators. :id and :retry persist across
     events per the SSE spec."
    (when (pos? (length state:data-lines))
      (on-event {:event (or state:event-type "message")
                 :data (string/join (freeze state:data-lines) "\n")
                 :id state:last-id
                 :retry state:retry}))
    (put state :event-type nil)
    (put state :data-lines @[]))

  (defn sse-handle-line [state line on-event]
    "Apply one line of an SSE stream to state. Empty line flushes the
     event; comments and unknown fields are ignored."
    (if (empty? line)
      (sse-dispatch-event state on-event)
      (let [parsed (sse-parse-field line)]
        (when parsed
          (case parsed:field
            "event" (put state :event-type parsed:value)
            "data" (push state:data-lines parsed:value)
            "id" (unless (string/contains? parsed:value "\0")
                   (put state :last-id parsed:value))
            "retry"
              (let [[ok? n] (protect (parse-int parsed:value))]
                (when (and ok? n (pos? n)) (put state :retry n))))))))

  (defn sse-for-each-event [t headers on-event]
    "Parse the SSE body on transport t, calling on-event for each
     complete event. Returns when the stream closes."
    (let [state @{:event-type nil :data-lines @[] :last-id nil :retry nil}]
      (sse-for-each-body-line t headers
                              (fn [line] (sse-handle-line state line on-event)))))

  (defn sse-open [url headers last-event-id]
    "Open an SSE request. Returns {:transport :status :headers}.
     Body is NOT consumed — caller parses it via sse-for-each-event."
    (let* [url-parsed (parse-url url)
           base-headers {:accept "text/event-stream" :cache-control "no-cache"}
           with-id (if last-event-id
                     (merge base-headers {:last-event-id last-event-id})
                     base-headers)
           user-headers (freeze (or headers {}))
           final-headers (merge with-id user-headers)
           t (open-transport url-parsed)]
      (write-request-line t "GET" url-parsed:path)
      (write-headers t
                     (build-request-headers url-parsed:host final-headers nil
                     false))
      (t-write t "\r\n")
      (t-flush t)
      (let* [status-line (read-status-line t)
             resp-headers (read-headers t)]
        {:transport t :status status-line:status :headers resp-headers})))

  (defn sse-get [url &named headers last-event-id @reconnect]
    "Open an SSE connection to url and return a fiber that yields
     events until the stream terminates. Each event is a struct:
       {:event \"message\" :data \"...\" :id \"...\" :retry 3000}

     :reconnect — true (default) or nil/false.
       When true, follows EventSource semantics: on disconnect or
       failure, waits retry-ms (last server-sent :retry, default 3000)
       and reopens with Last-Event-ID. Stops on HTTP 204 No Content.
     :last-event-id — initial Last-Event-ID header.
     :headers — extra request headers merged into the SSE defaults."
    (default reconnect true)
    (fiber/new (fn []
                 (def @current-id last-event-id)
                 (def @retry-ms sse-default-retry-ms)
                 (block :session
                   (forever
                     (let* [[ok? result] (protect (let [conn (sse-open url
                              headers current-id)]
                              (defer
                                (protect (t-close conn:transport))
                                (cond
                                  (= conn:status 204) :done
                                  (and (>= conn:status 200) (< conn:status 300))
                                    (begin
                                      (sse-for-each-event conn:transport
                                      conn:headers
                                      (fn [evt]
                                        (when evt:id (assign current-id evt:id))
                                        (when evt:retry
                                          (assign retry-ms evt:retry))
                                        (yield evt)))
                                      :eof)
                                  true (error {:error :http-error
                                  :reason :sse-bad-status
                                  :status conn:status
                                  :message "SSE: non-2xx response"})))))]
                       (cond
                         (and ok? (= result :done)) (break :session)
                         (not reconnect) (break :session))
                       (ev/sleep (/ retry-ms 1000.0)))))) |:yield|))

  (defn sse-post [url body &named headers]
    "POST to url with body, expecting a text/event-stream response.
     Returns a fiber that yields events until the server closes.
     Unlike sse-get this does NOT auto-reconnect — POST is typically
     non-idempotent (think LLM streaming: you don't want to silently
     re-submit a prompt).

     Use case: OpenAI-compatible /v1/chat/completions with
     {\"stream\": true} — the body is an SSE stream of token deltas
     terminated by a `data: [DONE]` sentinel the caller can recognize."
    (fiber/new (fn []
                 (let* [url-parsed (parse-url url)
                        base-headers {:accept "text/event-stream"
                                      :cache-control "no-cache"
                                      :content-type "application/json"}
                        user-headers (freeze (or headers {}))
                        final-headers (merge base-headers user-headers)
                        t (open-transport url-parsed)]
                   (defer
                     (protect (t-close t))
                     (write-request-line t "POST" url-parsed:path)
                     (write-headers t
                                    (build-request-headers url-parsed:host
                                    final-headers body false))
                     (t-write t "\r\n")
                     (unless (nil? body) (t-write t body))
                     (t-flush t)
                     (let* [status-line (read-status-line t)
                            resp-headers (read-headers t)]
                       (cond
                         (and (>= status-line:status 200)
                              (< status-line:status 300))
                           (sse-for-each-event t resp-headers
                           (fn [evt] (yield evt)))
                         true
                           (error {:error :http-error
                                   :reason :sse-bad-status
                                   :status status-line:status
                                   :body (read-body t resp-headers)
                                   :message "SSE POST: non-2xx response"}))))))
               |:yield|))

  (defn sse-format-field [field value]
    "Serialize one field of an SSE event. Data values with embedded
     newlines are emitted as repeated 'field: line' entries per spec."
    (let [s (string value)]
      (if (and (= field "data") (string/contains? s "\n"))
        (string/join (map (fn [line] (string/format "data: {}\n" line))
                          (string/split s "\n")) "")
        (string/format "{}: {}\n" field s))))

  (defn format-sse-event [evt]
    "Serialize an event struct to SSE wire format. Recognizes :event,
     :data, :id, :retry; unknown fields are omitted. Returns the
     complete frame, terminator included."
    (let [parts @[]]
      (when (and evt:event (not (= evt:event "message")))
        (push parts (sse-format-field "event" evt:event)))
      (when evt:id (push parts (sse-format-field "id" evt:id)))
      (when evt:retry (push parts (sse-format-field "retry" evt:retry)))
      (when evt:data (push parts (sse-format-field "data" evt:data)))
      (string (string/join (freeze parts) "") "\n")))

  (defn sse-response [body-fn &named headers]
    "Build a streaming SSE response. body-fn is a closure (fn [send-event])
     where send-event takes an event struct and emits it to the client.
     Returns a response that uses chunked transfer; suitable for use
     with http:serve."
    (let* [base-headers {:content-type "text/event-stream"
                         :cache-control "no-cache"
                         :connection "keep-alive"
                         :transfer-encoding "chunked"}
           merged (if (nil? headers)
                    base-headers
                    (merge base-headers (freeze headers)))]
      {:status 200
       :headers merged
       :body (fn [write-chunk]
               (body-fn (fn [evt] (write-chunk (format-sse-event evt)))))}))

  ## ── Tests ─────────────────────────────────────────────────────────────

  (defn run-tests []
    "Sanity checks on SSE field parsing, dispatch and formatting."
    # SSE: field parsing
    (let [f (sse-parse-field "event: update")]
      (assert (= f:field "event") "sse-parse-field event name"))
    (let [f (sse-parse-field "data: hello")]
      (assert (= f:value "hello")
              "sse-parse-field eats one leading space on value"))
    (let [f (sse-parse-field "data:hello")]
      (assert (= f:value "hello") "sse-parse-field no space is fine too"))
    (assert (nil? (sse-parse-field ": comment"))
            "sse-parse-field treats : prefix as comment")
    (assert (nil? (sse-parse-field "")) "sse-parse-field skips empty line")
    (let [f (sse-parse-field "field-no-colon")]
      (assert (= f:field "field-no-colon")
              "sse-parse-field colonless line is field")
      (assert (= f:value "") "sse-parse-field colonless value is empty"))

    # SSE: sse-handle-line dispatches events on blank lines
    (let [events @[]
          state @{:event-type nil :data-lines @[] :last-id nil :retry nil}]
      (defn collect [e]
        (push events e))
      (sse-handle-line state "event: ping" collect)
      (sse-handle-line state "data: one" collect)
      (sse-handle-line state "data: two" collect)
      (sse-handle-line state "id: 42" collect)
      (sse-handle-line state "" collect)  # dispatch
      (sse-handle-line state "data: alone" collect)
      (sse-handle-line state "" collect)  # dispatch (no event, inherits id)
      (assert (= (length events) 2) "sse-handle-line: 2 events")
      (let [e0 (get events 0)]
        (assert (= e0:event "ping") "SSE event name")
        (assert (= e0:data "one\ntwo") "SSE multi-line data joined")
        (assert (= e0:id "42") "SSE id captured"))
      (let [e1 (get events 1)]
        (assert (= e1:event "message") "SSE default event type")
        (assert (= e1:id "42") "SSE id persists across events")))

    # SSE: an id holding NUL is ignored, and any other id is kept.
    # The counter-factual is a check for the digit 0, which drops
    # `id: 10` and keeps an id that holds NUL.
    (let [events @[]
          state @{:event-type nil :data-lines @[] :last-id nil :retry nil}]
      (defn collect [e]
        (push events e))
      (sse-handle-line state "id: 10" collect)
      (sse-handle-line state "data: first" collect)
      (sse-handle-line state "" collect)
      (sse-handle-line state "id: 1\02" collect)
      (sse-handle-line state "data: second" collect)
      (sse-handle-line state "" collect)
      (assert (= (length events) 2) "SSE id: 2 events")
      (assert (= (get (get events 0) :id) "10")
              "SSE id holding the digit 0 is kept")
      (assert (= (get (get events 1) :id) "10") "SSE id holding NUL is ignored"))

    # SSE: format-sse-event round-trips basics
    (assert (= (format-sse-event {:event "message" :data "hi"}) "data: hi\n\n")
            "format-sse-event default event skips 'event:' line")
    (assert (= (format-sse-event {:event "tick" :data "1" :id "a"})
               "event: tick\nid: a\ndata: 1\n\n") "format-sse-event full frame")
    (assert (= (format-sse-event {:data "line1\nline2"})
               "data: line1\ndata: line2\n\n")
            "format-sse-event splits multi-line data")
    (assert (= (format-sse-event {:retry 5000}) "retry: 5000\n\n")
            "format-sse-event retry-only event")

    # SSE: parse + format round-trip
    (let [events @[]
          state @{:event-type nil :data-lines @[] :last-id nil :retry nil}
          wire (format-sse-event {:event "tick" :data "hello" :id "7"})]
      (each line in (string/split wire "\n")
        (sse-handle-line state line (fn [e] (push events e))))
      (assert (= (length events) 1) "SSE round-trip: one event")
      (let [e (get events 0)]
        (assert (= e:event "tick") "SSE round-trip: event")
        (assert (= e:data "hello") "SSE round-trip: data")
        (assert (= e:id "7") "SSE round-trip: id")))

    # SSE: sse-response builds a chunked streaming response
    (let [resp (sse-response (fn [send]
                               (send {:data "first"})
                               (send {:event "tick" :data "1"})))]
      (assert (= resp:status 200) "sse-response: status 200")
      (assert (= (get resp:headers :content-type) "text/event-stream")
              "sse-response: content-type")
      (assert (string/contains? (string/lowercase (get resp:headers
                                :transfer-encoding)) "chunked")
              "sse-response: transfer-encoding chunked")
      (assert (fn? resp:body) "sse-response: body is a closure"))
    true)

  {:get sse-get
   :post sse-post
   :response sse-response
   :format-event format-sse-event
   :test run-tests})
