(elle/epoch 13)
# audited: 2026-09-28
## The TCP and TLS transports that every HTTP wire helper reads and writes.
## lib/http/overview.md
##
## Loaded via:
##   (def transport ((import "std/http/transport") :tls tls))
##
## :tls is the std/tls module; without it, an https URL raises
## :tls-not-configured.
##
## Exports: {:tcp :tls :open :read :read-line :write :flush :close
##           :with-file :test}

(fn [&named tls]

  ## A transport is a struct of closures exposing the subset of port-like
  ## operations HTTP needs: {:read :read-line :write :flush :close}. All
  ## wire-format helpers take a transport, so the same code path handles
  ## plain TCP ports and TLS connections.

  (defn tcp-transport [port]
    "Wrap a plain TCP (or file) port as a transport.
     Writes are buffered in user space; flush sends a single port/write
     to avoid per-line scheduler yields on the io_uring path."
    (def @wbuf-parts @[])
    {:read (fn [n] (port/read port n))
     :read-line (fn [] (port/read-line port))
     :write (fn [data]
              (let [d (if (bytes? data) data (bytes data))]
                (push wbuf-parts d)))
     :flush (fn []
              (when (> (length wbuf-parts) 0)
                (let [combined (apply concat (freeze wbuf-parts))]
                  (port/write port combined)
                  (assign wbuf-parts @[]))))
     :close (fn [] (port/close port))})

  (defn strip-line-terminator [s]
    "Strip a trailing CRLF, LF, or CR from a line. CRLF is a single
     grapheme in Elle, so all three cases drop one grapheme."
    (if (and s
             (or (string/ends-with? s "\r\n") (string/ends-with? s "\n")
                 (string/ends-with? s "\r")))
      (slice s 0 (dec (length s)))
      s))

  (defn tls-transport [conn]
    "Wrap a TLS connection (from the configured tls module) as a transport.
     Only available when :tls was passed to the module initializer.

     Note: port/read-line strips trailing newlines; tls:read-line does
     not. We normalize here so the wire-format helpers see the same
     semantics regardless of transport."
    {:read (fn [n] (tls:read conn n))
     :read-line (fn []
                  (let [line (tls:read-line conn)]
                    (when line (strip-line-terminator line))))
     :write (fn [data] (tls:write conn data))
     :flush (fn [] nil)
     :close (fn [] (tls:close conn))})

  (defn open-transport [url-parsed]
    "Open a transport to the URL's host:port. Uses TLS when the scheme is
     https and a tls module was supplied to the module initializer.
     Signals :http-error :tls-not-configured if an https URL is used
     without one."
    (cond
      (= url-parsed:scheme "https")
        (begin
          (when (nil? tls)
            (error {:error :http-error
                    :reason :tls-not-configured
                    :url url-parsed
                    :message "https URL requires the std/tls module; pass it as :tls to (import \"std/http\")"}))
          (tls-transport (tls:connect url-parsed:host url-parsed:port)))
      true (tcp-transport (tcp/connect url-parsed:host url-parsed:port))))

  (defn t-read [t n]
    ((get t :read) n))
  (defn t-read-line [t]
    ((get t :read-line)))
  (defn t-write [t data]
    ((get t :write) data))
  (defn t-flush [t]
    ((get t :flush)))
  (defn t-close [t]
    ((get t :close)))

  (defn with-file-transport [path mode thunk]
    "Open the file at path as a transport, run (thunk transport), and close
     the file. The submodules' tests read and write their wire formats
     through it."
    (let [p (port/open path mode)]
      (defer
        (protect (port/close p))
        (thunk (tcp-transport p)))))

  ## ── Tests ─────────────────────────────────────────────────────────────

  (defn run-tests []
    "Sanity checks on the transports' line handling."
    # strip-line-terminator: parity between tcp and tls transports.
    # port/read-line strips newlines; tls:read-line does not; tls-transport
    # reconciles them so wire-format helpers see a single semantics.
    (assert (= (strip-line-terminator "hi\r\n") "hi")
            "strip-line-terminator CRLF")
    (assert (= (strip-line-terminator "hi\n") "hi") "strip-line-terminator LF")
    (assert (= (strip-line-terminator "hi\r") "hi") "strip-line-terminator CR")
    (assert (= (strip-line-terminator "hi") "hi")
            "strip-line-terminator no terminator")
    (assert (= (strip-line-terminator "") "") "strip-line-terminator empty")
    (assert (nil? (strip-line-terminator nil))
            "strip-line-terminator nil passthrough")
    true)

  {:tcp tcp-transport
   :tls tls-transport
   :open open-transport
   :read t-read
   :read-line t-read-line
   :write t-write
   :flush t-flush
   :close t-close
   :with-file with-file-transport
   :test run-tests})
