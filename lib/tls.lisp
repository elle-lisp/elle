(elle/epoch 13)
# audited: 2026-09-28
## TLS connections over TCP ports, with the elle-tls plugin's state machine driven from Elle.
## lib/tls.md
##
## The file's closure takes the plugin struct and returns the export struct:
##   (def tls ((import "std/tls") (import "plugin/tls")))
##   (let [conn (tls:connect "example.com" 443)]
##     (defer (tls:close conn) ...))
##
## A tls-conn is {:tcp port :tls tls-state}: the TCP port, and the TlsState
## external from the elle-tls plugin. Every read and write goes through the
## TCP port and the fiber scheduler, so each function here runs inside a
## scheduler context.

(fn [plugin]
  ## Extract plugin primitives from the struct so they can be called
  ## as local bindings. Plugin primitives are not resolvable by name
  ## at compile time — they must be accessed through the struct.
  (def process-fn (get plugin :process))
  (def get-outgoing-fn (get plugin :get-outgoing))
  (def handshake-complete?-fn (get plugin :handshake-complete?))
  (def client-state-fn (get plugin :client-state))
  (def server-state-fn (get plugin :server-state))
  (def server-config-fn (get plugin :server-config))
  (def alpn-protocol-fn (get plugin :alpn-protocol))

  ## ── Private: handshake driver ───────────────────────────────────────────

  (defn tls-handshake [port tls]
    "Drive TLS handshake to completion over a TCP port.
     Mutates tls in place. Returns nil on success.
     Must be called inside a scheduler context (the async scheduler).

     Loop invariant:
       - After every tls/process call, drain and send outgoing bytes.
         TLS 1.3 may produce post-handshake messages at any time.
       - Check handshake-complete? AFTER sending outgoing — the server
         needs to receive our Finished before it considers us ready."
    # Pump the state machine with empty bytes to generate the initial
    # ClientHello (client side) or enter the wait state (server side).
    (process-fn tls (bytes))
    (forever
      # INVARIANT: Send any queued ciphertext before doing anything else.
      # This must happen on the first iteration for ClientHello (client side)
      # and after every subsequent process call.
      (let [out (get-outgoing-fn tls)]
        (when (> (length out) 0) (port/write port out)))  # async — yields SIG_IO

      # If handshake is complete, we're done.
      (when (handshake-complete?-fn tls) (break nil))

      # Read more ciphertext from the network.
      # Note: TCP ports use binary encoding; port/read returns bytes.
      (let [data (port/read port 16384)]
        (when (nil? data)
          (error {:error :tls-error
                  :reason :connection-closed
                  :phase :handshake
                  :message "connection closed during handshake"}))

        # Feed into state machine. Outgoing data from this call
        # will be sent at the top of the next loop iteration.
        (process-fn tls data))))

  ## ── Public: connection functions ────────────────────────────────────────

  (defn tls/connect [hostname port-num & args]
    "Connect to a TLS server. Returns a tls-conn struct {:tcp port :tls tls-state}.
     Must be called inside a scheduler context (the async scheduler).

     hostname is used for TLS SNI and certificate verification, and is passed
     to tcp/connect, which resolves it and tries each address for the TCP
     connection.

     Optional third argument opts struct:
       :no-verify  bool   — skip certificate verification (dev/test only)
       :ca-file    string — path to PEM CA bundle
       :client-cert string — path to PEM client certificate chain
       :client-key  string — path to PEM client private key"
    (let* [opts (or (get args 0) {})
           tcp-port (tcp/connect hostname port-num)  # async; resolves hostname
           tls (client-state-fn hostname opts)]
      (let [[ok? result] (protect (tls-handshake tcp-port tls))]
        (unless ok?
          # Handshake failed. Close TCP port before re-raising.
          # Do not attempt to send close_notify — the connection is broken.
          (port/close tcp-port)
          (error result))
        {:tcp tcp-port :tls tls})))

  (defn tls/accept [listener config]
    "Accept a TLS connection on a TCP listener. Returns a tls-conn struct.
     listener: a TcpListener port from (tcp/listen host port).
     config:   a tls-server-config from (tls/server-config cert key).
     Must be called inside a scheduler context."
    (let* [tcp-port (tcp/accept listener)  # async
           tls (server-state-fn config)]
      (let [[ok? result] (protect (tls-handshake tcp-port tls))]
        (unless ok?
          (port/close tcp-port)
          (error result))
        {:tcp tcp-port :tls tls})))

  ## ── Private: additional plugin primitives ──────────────────────────────
  ## Extracted here so data-transfer functions close over them without
  ## reaching into `plugin` at each call site.
  (def read-plaintext-fn (get plugin :read-plaintext))
  (def get-plaintext-fn (get plugin :get-plaintext))
  (def write-plaintext-fn (get plugin :write-plaintext))
  (def plaintext-indexof-fn (get plugin :plaintext-indexof))
  (def close-notify-fn (get plugin :close-notify))

  ## ── Public: data transfer ─────────────────────────────────────────────────

  (defn tls/read [conn n]
    "Read up to n bytes of decrypted plaintext from a TLS connection.
     Returns bytes, or nil on EOF (connection closed by peer).
     Must be called inside a scheduler context."
    (let [tls conn:tls
          port conn:tcp]
      (forever
        # Check buffered plaintext first: it saves a network round-trip.
        (let [buffered (read-plaintext-fn tls n)]
          (when (> (length buffered) 0) (break buffered)))
        # The plaintext buffer is empty, so read from the network. 16384 is
        # the TLS maximum record size.
        (let [data (port/read port 16384)]
          (when (nil? data)
            # TCP closed. process-fn may have buffered plaintext from
            # a segment that also contained close_notify. One final drain.
            (let [final (read-plaintext-fn tls n)]
              (break (if (> (length final) 0) final nil))))
          (process-fn tls data)
          # INVARIANT: Send outgoing after every tls/process.
          # TLS 1.3 post-handshake messages (NewSessionTicket, KeyUpdate) must
          # be sent or the connection stalls.
          (let [out (get-outgoing-fn tls)]
            (when (> (length out) 0) (port/write port out)))))))

  (defn tls/read-line [conn]
    "Read a line (through \\n, byte 10) from a TLS connection.
     Returns a string including the newline, or nil on EOF.
     Uses tls/plaintext-indexof to scan without draining, then
     tls/read-plaintext to drain exactly the right number of bytes.
     Must be called inside a scheduler context."
    (let [tls conn:tls
          port conn:tcp
          chunks @[]]
      (forever
        # Scan the buffered plaintext for a newline, without draining it.
        (let [idx (plaintext-indexof-fn tls 10)]
          (when (not (nil? idx))
            # Found a newline at position idx.
            # Drain exactly (idx + 1) bytes — up to and including the newline.
            (let [line-bytes (read-plaintext-fn tls (+ idx 1))]
              (push chunks (string line-bytes))
              # Remainder (bytes after the newline) stays in the plaintext buffer
              # for the next tls/read-line call.
              (break (apply concat chunks)))))
        # No newline in the buffer yet, so read more from the network.
        (let [data (port/read port 16384)]
          # At EOF, return what has accumulated, or nil if nothing has.
          (when (nil? data)
            (let [remaining (get-plaintext-fn tls)]
              (when (> (length remaining) 0) (push chunks (string remaining)))
              (break (if (> (length chunks) 0) (apply concat chunks) nil))))
          (process-fn tls data)
          # INVARIANT: Send outgoing after every tls/process.
          (let [out (get-outgoing-fn tls)]
            (when (> (length out) 0) (port/write port out)))))))  # async

  (defn tls/read-all [conn]
    "Read all remaining decrypted bytes until EOF. Returns bytes.
     Returns empty bytes if connection is already at EOF.
     Must be called inside a scheduler context."
    (let [tls conn:tls
          port conn:tcp
          chunks @[]]
      (forever
        (let [data (port/read port 16384)]
          # At EOF, drain any remaining plaintext and return what has
          # accumulated.
          (when (nil? data)
            (let [remaining (get-plaintext-fn tls)]
              (when (> (length remaining) 0) (push chunks remaining)))
            (break (if (> (length chunks) 0)
                     (apply concat (freeze chunks))
                     (bytes))))
          (process-fn tls data)
          # INVARIANT: Send outgoing after every tls/process.
          (let [out (get-outgoing-fn tls)]
            (when (> (length out) 0) (port/write port out)))  # async

          # Accumulate any newly decrypted plaintext.
          (let [pt (get-plaintext-fn tls)]
            (when (> (length pt) 0) (push chunks pt)))))))

  (defn tls/write [conn data]
    "Encrypt data and send over TLS. data may be bytes or string.
     Returns the number of plaintext bytes written.
     Must be called inside a scheduler context."
    (let* [tls conn:tls
           port conn:tcp
           plaintext (if (string? data) (bytes data) data)
           result (write-plaintext-fn tls plaintext)]
      (when (= result:status :error)
        (error {:error :tls-error :reason :write-failed :message result:message}))
      (let [out result:outgoing]
        (when (> (length out) 0) (port/write port out)))  # async
      (length plaintext)))

  (defn tls/close [conn]
    "Close a TLS connection. Sends a TLS close_notify alert then closes the TCP port.
     Complies with RFC 8446 §6.1: each party must send close_notify before
     closing its write side. Returns nil."
    (let* [notify-result (close-notify-fn conn:tls)
           outgoing notify-result:outgoing]
      (when (> (length outgoing) 0) (port/write conn:tcp outgoing))
      (port/close conn:tcp))
    nil)

  ## ── Public: stream constructors ───────────────────────────────────────────
  ##
  ## These return fibers — Elle's universal stream type.
  ## All stream/map, stream/filter, stream/collect, stream/take, etc. work
  ## on these because they operate on fibers via fiber/resume, stream/done?,
  ## fiber/value — not on ports.

  (defn tls/lines [conn]
    "Return a fiber that yields lines from a TLS connection one at a time.
     Closes the connection when the stream is exhausted.
     Compose with stream/map, stream/filter, stream/take, stream/collect, etc.
     Must be called inside a scheduler context."
    (fiber/new (fn []
                 (forever
                   (let [line (tls/read-line conn)]
                     (if (nil? line)
                       (begin
                         (tls/close conn)
                         (break))
                       (yield line))))) |:yield|))

  (defn tls/chunks [conn size]
    "Return a fiber that yields byte chunks of `size` from a TLS connection.
     Final chunk may be smaller. Closes the connection when exhausted.
     Must be called inside a scheduler context."
    (fiber/new (fn []
                 (forever
                   (let [chunk (tls/read conn size)]
                     (if (nil? chunk)
                       (begin
                         (tls/close conn)
                         (break))
                       (yield chunk))))) |:yield|))

  (defn tls/writer [conn]
    "Return a write-stream fiber. Resume with bytes/string to write.
     Resume with nil to close the connection.
     Must be called inside a scheduler context."
    (fiber/new (fn []
                 (forever
                   (let [val (yield nil)]
                     (if (nil? val)
                       (begin
                         (tls/close conn)
                         (break))
                       (tls/write conn val))))) |:yield|))

  ## ── Public: ALPN protocol query ──────────────────────────────────────

  (defn tls/alpn-protocol [conn]
    "Return the negotiated ALPN protocol string (e.g. \"h2\"), or nil."
    (alpn-protocol-fn conn:tls))

  ## ── Export struct ──────────────────────────────────────────────────────
  {:connect tls/connect
   :accept tls/accept
   :server-config server-config-fn
   :read tls/read
   :read-line tls/read-line
   :read-all tls/read-all
   :write tls/write
   :close tls/close
   :lines tls/lines
   :chunks tls/chunks
   :writer tls/writer
   :alpn-protocol tls/alpn-protocol})
