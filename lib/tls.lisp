## lib/tls.lisp — TLS client and server for Elle
##
## TLS client and server using the elle-tls plugin for state machine management.
## All socket I/O is async via native TCP ports and the fiber scheduler.
##
## Dependencies:
##   - elle-tls plugin loaded via (import-file "path/to/libelle_tls.so")
##   - tcp/connect, tcp/accept from net primitives
##   - port/read, port/write from stream primitives  (port/read returns
##     bytes for TCP ports — TCP streams use binary encoding in this runtime)
##   - ev/spawn from scheduler
##   - port/close for TCP port lifecycle
##   - subprocess/system for hostname resolution (getent fallback)
##
## Usage:
##   (def tls-plugin (import-file "target/release/libelle_tls.so"))
##   (def tls ((import-file "lib/tls.lisp") tls-plugin))
##   (let [[conn (tls:connect "example.com" 443)]]
##     (defer (tls:close conn) ...))
##
## The file exports a function that accepts the plugin struct and returns the
## public API struct. The plugin struct is closed over so all public
## functions can call plugin primitives without naming them globally.
##
## API change from spec: the entry point takes the plugin struct as argument.
## Load with: (def tls ((import-file "lib/tls.lisp") tls-plugin))
##
## tls-conn shape:
##   {:tcp port :tls tls-state}
##   :tcp — the underlying TcpStream port
##   :tls — the TlsState ExternalObject from the elle-tls plugin

## ── Hostname resolution ─────────────────────────────────────────────────────
##
## The io_uring TCP backend requires IP addresses for tcp/connect. Hostnames
## must be resolved before connecting. We use getent(1) via subprocess to
## delegate to the system resolver (glibc getaddrinfo), which handles /etc/hosts,
## systemd-resolved, and any configured DNS forwarder.

(defn first-word [s]
  "Extract first whitespace-delimited token from string s."
  (let [[sp (string/find s " ")]]
    (if (nil? sp) s (slice s 0 sp))))

(defn first-line [s]
  "Extract first line from string s."
  (let [[nl (string/find s "\n")]]
    (if (nil? nl) s (slice s 0 nl))))

(defn resolve-host [host]
  "Resolve hostname to an IP address string for use with tcp/connect.
   Uses sys/resolve (getaddrinfo) for system-correct resolution that
   consults /etc/hosts and nsswitch.conf. Returns the first IP address."
  (first (sys/resolve host)))

## ── The entry-point thunk ───────────────────────────────────────────────────
##
## The file's last expression is a function that accepts the plugin struct
## and returns the public API. Call it like:
##   (def tls ((import-file "lib/tls.lisp") tls-plugin))

(fn [plugin]
  ## Extract plugin primitives from the struct so they can be called
  ## as local bindings. Plugin primitives are not resolvable by name
  ## at compile time — they must be accessed through the struct.
  (def process-fn             (get plugin :process))
  (def get-outgoing-fn        (get plugin :get-outgoing))
  (def handshake-complete?-fn (get plugin :handshake-complete?))
  (def client-state-fn        (get plugin :client-state))
  (def server-state-fn        (get plugin :server-state))
  (def server-config-fn       (get plugin :server-config))

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
      (let [[out (get-outgoing-fn tls)]]
        (when (> (length out) 0)
          (port/write port out)))          # async — yields SIG_IO

      # If handshake is complete, we're done.
      (when (handshake-complete?-fn tls)
        (break nil))

      # Read more ciphertext from the network.
      # Note: TCP ports use binary encoding; port/read returns bytes.
      (let [[data (port/read port 16384)]]  # async — yields SIG_IO
        (when (nil? data)
          (error {:error :tls-error
                  :message "tls: connection closed during handshake"}))

        # Feed into state machine. Outgoing data from this call
        # will be sent at the top of the next loop iteration.
        (process-fn tls data))))

  ## ── Public: connection functions ────────────────────────────────────────

  (defn tls/connect [hostname port-num & args]
    "Connect to a TLS server. Returns a tls-conn struct {:tcp port :tls tls-state}.
     Must be called inside a scheduler context (the async scheduler).

     hostname is used for TLS SNI and certificate verification.
     The hostname is DNS-resolved to an IP before connecting (required by
     the io_uring backend; the sync backend supports hostname connects directly).

     Optional third argument opts struct:
       :no-verify  bool   — skip certificate verification (dev/test only)
       :ca-file    string — path to PEM CA bundle
       :client-cert string — path to PEM client certificate chain
       :client-key  string — path to PEM client private key"
    (let* [[opts (or (get args 0) {})]
           # Resolve hostname to IP. The io_uring TCP backend requires an IP
           # address; hostnames must be resolved before calling tcp/connect.
           # SNI and cert verification still use the original hostname.
           [ip (resolve-host hostname)]
           [tcp-port (tcp/connect ip port-num)]       # async
           [tls (client-state-fn hostname opts)]]     # sync — state machine only
      (let [[[ok? result] (protect (tls-handshake tcp-port tls))]]
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
    (let* [[tcp-port (tcp/accept listener)]       # async
           [tls (server-state-fn config)]]        # sync
      (let [[[ok? result] (protect (tls-handshake tcp-port tls))]]
        (unless ok?
          (port/close tcp-port)
          (error result))
        {:tcp tcp-port :tls tls})))

  ## ── Private: additional plugin primitives ──────────────────────────────
  ## Extracted here so data-transfer functions close over them without
  ## reaching into `plugin` at each call site.
  (def read-plaintext-fn    (get plugin :read-plaintext))
  (def get-plaintext-fn     (get plugin :get-plaintext))
  (def write-plaintext-fn   (get plugin :write-plaintext))
  (def plaintext-indexof-fn (get plugin :plaintext-indexof))
  (def close-notify-fn      (get plugin :close-notify))

  ## ── Public: data transfer ─────────────────────────────────────────────────

  (defn tls/read [conn n]
    "Read up to n bytes of decrypted plaintext from a TLS connection.
     Returns bytes, or nil on EOF (connection closed by peer).
     Must be called inside a scheduler context."
    (let [[tls conn:tls]
          [port conn:tcp]]
      # Single loop: break immediately if plaintext is already buffered,
      # otherwise read from the network until we get data or EOF.
      (forever
        # Check buffered plaintext first — avoid a network round-trip if data is ready.
        (let [[buffered (read-plaintext-fn tls n)]]
          (when (> (length buffered) 0)
            (break buffered)))
        # Plaintext buffer empty — read from network.
        (let [[data (port/read port 16384)]]  # async
          (when (nil? data) (break nil))         # EOF
          (process-fn tls data)
          # INVARIANT: Send outgoing after every tls/process.
          # TLS 1.3 post-handshake messages (NewSessionTicket, KeyUpdate) must
          # be sent or the connection stalls.
          (let [[out (get-outgoing-fn tls)]]
            (when (> (length out) 0)
              (port/write port out)))))))

  (defn tls/read-line [conn]
    "Read a line (through \\n, byte 10) from a TLS connection.
     Returns a string including the newline, or nil on EOF.
     Uses tls/plaintext-indexof to scan without draining, then
     tls/read-plaintext to drain exactly the right number of bytes.
     Must be called inside a scheduler context."
    (let [[tls conn:tls]
          [port conn:tcp]
          [chunks @[]]]     # accumulated string fragments before the newline
      (forever
        # Scan for newline in the buffered plaintext — do NOT drain yet.
        (let [[idx (plaintext-indexof-fn tls 10)]]   # 10 = \n
          (when (not (nil? idx))
            # Found a newline at position idx.
            # Drain exactly (idx + 1) bytes — up to and including the newline.
            (let [[line-bytes (read-plaintext-fn tls (+ idx 1))]]
              (push chunks (string line-bytes))
              # Remainder (bytes after the newline) stays in the plaintext buffer
              # for the next tls/read-line call.
              (break (apply concat chunks)))))
        # No newline in buffer yet — read more from network.
        (let [[data (port/read port 16384)]]
          (when (nil? data)
            # EOF. Return whatever we have accumulated, or nil if nothing.
            (let [[remaining (get-plaintext-fn tls)]]
              (when (> (length remaining) 0)
                (push chunks (string remaining)))
              (break (if (> (length chunks) 0)
                       (apply concat chunks)
                       nil))))
          (process-fn tls data)
          # INVARIANT: Send outgoing after every tls/process.
          (let [[out (get-outgoing-fn tls)]]
            (when (> (length out) 0)
              (port/write port out)))))))   # async

  (defn tls/read-all [conn]
    "Read all remaining decrypted bytes until EOF. Returns bytes.
     Returns empty bytes if connection is already at EOF.
     Must be called inside a scheduler context."
    (let [[tls conn:tls]
          [port conn:tcp]
          [chunks @[]]]
      (forever
        (let [[data (port/read port 16384)]]
          (when (nil? data)
            # EOF. Drain any remaining plaintext and return accumulated data.
            (let [[remaining (get-plaintext-fn tls)]]
              (when (> (length remaining) 0)
                (push chunks remaining)))
            (break (if (> (length chunks) 0)
                     (apply concat (freeze chunks))
                     (bytes))))
          (process-fn tls data)
          # INVARIANT: Send outgoing after every tls/process.
          (let [[out (get-outgoing-fn tls)]]
            (when (> (length out) 0)
              (port/write port out)))         # async
          # Accumulate any newly decrypted plaintext.
          (let [[pt (get-plaintext-fn tls)]]
            (when (> (length pt) 0)
              (push chunks pt)))))))

  (defn tls/write [conn data]
    "Encrypt data and send over TLS. data may be bytes or string.
     Returns the number of plaintext bytes written.
     Must be called inside a scheduler context."
    (let* [[tls conn:tls]
           [port conn:tcp]
           [plaintext (if (string? data) (bytes data) data)]
           [result (write-plaintext-fn tls plaintext)]]
      (when (= result:status :error)
        (error {:error :tls-error :message result:message}))
      (let [[out result:outgoing]]
        (when (> (length out) 0)
          (port/write port out)))             # async
      (length plaintext)))

  (defn tls/close [conn]
    "Close a TLS connection. Sends a TLS close_notify alert then closes the TCP port.
     Complies with RFC 8446 §6.1: each party must send close_notify before
     closing its write side. Returns nil."
    (let* [[notify-result (close-notify-fn conn:tls)]
           [outgoing notify-result:outgoing]]
      (when (> (length outgoing) 0)
        (port/write conn:tcp outgoing))
      (port/close conn:tcp))
    nil)

  ## ── Public: stream constructors ───────────────────────────────────────────
  ##
  ## These return coroutines — Elle's universal stream type.
  ## All stream/map, stream/filter, stream/collect, stream/take, etc. work
  ## on these because they operate on coroutines via coro/resume, coro/done?,
  ## coro/value — not on ports.

  (defn tls/lines [conn]
    "Return a coroutine that yields lines from a TLS connection one at a time.
     Closes the connection when the stream is exhausted.
     Compose with stream/map, stream/filter, stream/take, stream/collect, etc.
     Must be called inside a scheduler context."
    (coro/new (fn []
      (forever
        (let [[line (tls/read-line conn)]]
          (if (nil? line)
            (begin (tls/close conn) (break))
            (yield line)))))))

  (defn tls/chunks [conn size]
    "Return a coroutine that yields byte chunks of `size` from a TLS connection.
     Final chunk may be smaller. Closes the connection when exhausted.
     Must be called inside a scheduler context."
    (coro/new (fn []
      (forever
        (let [[chunk (tls/read conn size)]]
          (if (nil? chunk)
            (begin (tls/close conn) (break))
            (yield chunk)))))))

  (defn tls/writer [conn]
    "Return a write-stream coroutine. Resume with bytes/string to write.
     Resume with nil to close the connection.
     Must be called inside a scheduler context."
    (coro/new (fn []
      (forever
        (let [[val (yield nil)]]
          (if (nil? val)
            (begin (tls/close conn) (break))
            (tls/write conn val)))))))

  ## ── Export struct ──────────────────────────────────────────────────────
  {:connect       tls/connect
   :accept        tls/accept
   :server-config server-config-fn
   :read          tls/read
   :read-line     tls/read-line
   :read-all      tls/read-all
   :write         tls/write
   :close         tls/close
   :lines         tls/lines
   :chunks        tls/chunks
   :writer        tls/writer})
