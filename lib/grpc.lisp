(elle/epoch 9)
## lib/grpc.lisp — gRPC client for Elle
##
## Layers on lib/http2 for transport; adds gRPC framing and trailer handling.
##
## Usage:
##   (def pb    (import "plugin/protobuf"))
##   (def http2 ((import "std/http2")))
##   (def grpc  ((import "std/grpc") :http2 http2 :protobuf pb))
##
##   (def schema (pb:schema (port/read-all (port/open "service.proto" :read))))
##   (def conn   (grpc:connect "/run/user/1000/myservice.sock"))
##   (def resp   (grpc:call-decode conn schema "/pkg.Svc/Method"
##                  "pkg.Request" {} "pkg.Response"))
##   (grpc:close conn)


(fn [&named http2 protobuf]

  ## ── gRPC message framing ─────────────────────────────────────────────
  ## Each gRPC message: 1 byte compressed flag + 4 byte big-endian length + payload

  (defn grpc-encode [message-bytes]
    "Wrap protobuf bytes in gRPC length-prefixed frame."
    (let [len (length message-bytes)]
      (concat (bytes 0                              # not compressed
                     (bit/shr len 24)
                     (bit/and (bit/shr len 16) 0xff)
                     (bit/and (bit/shr len 8) 0xff)
                     (bit/and len 0xff))
              message-bytes)))

  (defn grpc-decode [frame-bytes]
    "Extract protobuf bytes from gRPC length-prefixed frame.
     Returns the payload bytes, or nil if frame is empty."
    (when (and frame-bytes (>= (length frame-bytes) 5))
      (let [len (bit/or (bit/shl (get frame-bytes 1) 24)
                        (bit/shl (get frame-bytes 2) 16)
                        (bit/shl (get frame-bytes 3) 8)
                        (get frame-bytes 4))]
        (when (>= (length frame-bytes) (+ 5 len))
          (slice frame-bytes 5 (+ 5 len))))))

  ## ── Connect ────────────────────────────────────────────────────────

  (def h2-transport ((import "std/http2/transport")))

  (defn grpc-connect [socket-path]
    "Connect to a gRPC server over a Unix socket. Returns an h2 session."
    (let [transport (h2-transport:tcp (unix/connect socket-path))]
      (http2:connect nil :transport transport)))

  ## ── Collect gRPC response from stream ──────────────────────────────

  (defn check-grpc-status [headers]
    "Check grpc-status in headers/trailers. Raises on non-zero status."
    (let [status-pair (first (filter (fn [h] (= (get h 0) "grpc-status")) headers))]
      (when (and status-pair (not (= (get status-pair 1) "0")))
        (let [msg-pair (first (filter (fn [h] (= (get h 0) "grpc-message")) headers))]
          (error {:error :grpc-error
                  :code (parse-int (get status-pair 1))
                  :message (if msg-pair (get msg-pair 1) "unknown error")})))))

  (defn collect-grpc-response [s]
    "Read data + trailers from an h2 stream. Returns raw gRPC frame bytes.
     Raises on grpc-status != 0. Handles both trailers-only and
     headers+data+trailers gRPC responses."
    (let [@resp-headers nil
          @resp-data @[]
          @done false]
      (while (not done)
        (let [msg (s:data-queue:take)]
          (match msg:type
            :headers (begin
                       (if (nil? resp-headers)
                         (begin
                           (assign resp-headers msg:headers)
                           ## Trailers-only: check grpc-status on initial headers
                           (when msg:end-stream
                             (check-grpc-status msg:headers)))
                         ## Trailers after data
                         (check-grpc-status msg:headers))
                       (when msg:end-stream (assign done true)))
            :data    (begin (push resp-data msg:data)
                            (when msg:end-stream (assign done true)))
            :rst     (error {:error :grpc-error :reason :stream-reset
                             :code msg:code})
            :error   (error msg:error)
            _        (assign done true))))
      (let [all-data (if (empty? resp-data)
                       (bytes)
                       (apply concat (freeze resp-data)))]
        (grpc-decode all-data))))

  ## ── Unary RPC call ─────────────────────────────────────────────────

  (defn grpc-call [session schema method request-type request-struct]
    "Make a unary gRPC call. Returns raw protobuf bytes of response."
    (let* [body (grpc-encode (protobuf:encode schema request-type request-struct))
           stream (http2:send-raw session "POST" method
                    :body body
                    :headers [["content-type" "application/grpc"]
                              ["te" "trailers"]])]
      (collect-grpc-response stream)))

  (defn grpc-call-decode [session schema method request-type request-struct response-type]
    "Make a unary gRPC call and decode the response.
     Returns the decoded Elle struct."
    (let [raw (grpc-call session schema method request-type request-struct)]
      (if (nil? raw)
        {}
        (protobuf:decode schema response-type raw))))

  ## ── Server-streaming RPC ──────────────────────────────────────────

  (defn grpc-call-stream [session schema method request-type request-struct response-type]
    "Open a server-streaming gRPC call. Returns a reader function.
     Call (reader) repeatedly; returns the next decoded message, or nil
     at end-of-stream. Each call blocks until a message is available.
     gRPC frames may span h2 DATA boundaries — the reader buffers and
     splits on the 5-byte length-prefixed frame headers."
    (def body (grpc-encode (protobuf:encode schema request-type request-struct)))
    (def s (http2:send-raw session "POST" method
              :body body
              :headers [["content-type" "application/grpc"]
                        ["te" "trailers"]]))
    (def @buf (bytes))
    (def @done false)

    ## Return reader closure
    (fn []
      ## Try to extract a complete gRPC frame from buf
      (def @result nil)
      (when (>= (length buf) 5)
        (let [len (bit/or (bit/shl (get buf 1) 24)
                          (bit/shl (get buf 2) 16)
                          (bit/shl (get buf 3) 8)
                          (get buf 4))
              frame-end (+ 5 len)]
          (when (>= (length buf) frame-end)
            (let [payload (slice buf 5 frame-end)]
              (assign buf (slice buf frame-end))
              (assign result (protobuf:decode schema response-type payload))))))

      ## If nothing in buffer, read h2 frames until we get a complete message
      (while (and (nil? result) (not done))
        (let [msg (s:data-queue:take)]
          (match msg:type
            :headers (begin
                       (check-grpc-status msg:headers)
                       (when msg:end-stream (assign done true)))
            :data    (begin
                       (assign buf (concat buf msg:data))
                       (when msg:end-stream (assign done true))
                       (when (and (nil? result) (>= (length buf) 5))
                         (let [len (bit/or (bit/shl (get buf 1) 24)
                                           (bit/shl (get buf 2) 16)
                                           (bit/shl (get buf 3) 8)
                                           (get buf 4))
                               frame-end (+ 5 len)]
                           (when (>= (length buf) frame-end)
                             (let [payload (slice buf 5 frame-end)]
                               (assign buf (slice buf frame-end))
                               (assign result (protobuf:decode schema response-type payload)))))))
            :rst     (error {:error :grpc-error :reason :stream-reset
                             :code msg:code})
            :error   (error msg:error)
            _        (assign done true))))
      result))

  ## ── Close ──────────────────────────────────────────────────────────

  (defn grpc-close [session]
    "Close a gRPC connection gracefully."
    (http2:close session))

  ## ── Exports ────────────────────────────────────────────────────────

  {:connect      grpc-connect
   :call         grpc-call
   :call-decode  grpc-call-decode
   :call-stream  grpc-call-stream
   :close        grpc-close
   :encode       grpc-encode
   :decode       grpc-decode})
