(elle/epoch 15)
# audited: 2026-11-12
## The TCP and TLS transports, each a framer and a gather over its connection, that every HTTP wire helper reads and writes.
## docs/stdlib/http/overview.md
##
## Loaded via:
##   (def transport ((import "std/http/transport") :tls tls))
##
## :tls is the std/tls module; without it, an https URL raises
## :tls-not-configured.
##
## Exports: {:tcp :tls :open :read-line :exact :write :flush :close
##           :with-file :test}

(fn [&named tls]
  (def framing ((import "std/framing")))

  ## A transport is a struct of closures over one framer and one gather:
  ## {:read-line :exact :write :flush :close}. Every wire helper takes a
  ## transport, so the same code path serves plain TCP ports and TLS
  ## connections. The framer holds the connection's unread bytes, so a
  ## connection is read through its transport alone. A write queues its
  ## value in the gather, and a flush sends the queue in one write.

  (defn read-line-of [f]
    "The next line of framer f as a string, or nil at the end."
    (let [line (framing:line f)]
      (when line (string line))))

  (defn transport-over [f g close]
    "The transport that reads through framer f, writes through gather g, and
     closes with close."
    {:read-line (fn [] (read-line-of f))
     :exact (fn [n] (framing:exact f n))
     :write (fn [data] (framing:write g data))
     :flush (fn [] (framing:flush g))
     :close close})

  (defn tcp-transport [port]
    "Wrap a binary TCP (or file) port as a transport. A flush sends the
     queued values in one gathered write, each from where it lies."
    (transport-over (framing:open port) (framing:gather port)
                    (fn [] (port/close port))))

  (defn tls-transport [conn]
    "Wrap a TLS connection (from the configured tls module) as a transport.
     A flush joins the queued values into one tls:write. Only available when
     :tls was passed to the module initializer."
    (transport-over (framing:open (fn [n] (tls:read conn n)))
                    (framing:gather (fn [parts]
                                      (tls:write conn (framing:join parts))))
                    (fn [] (tls:close conn))))

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

  (defn t-read-line [t]
    ((get t :read-line)))
  (defn t-exact [t n]
    ((get t :exact) n))
  (defn t-write [t data]
    ((get t :write) data))
  (defn t-flush [t]
    ((get t :flush)))
  (defn t-close [t]
    ((get t :close)))

  (defn with-file-transport [path mode thunk]
    "Open the file at path as a binary port wrapped in a transport, run
     (thunk transport), and close the file. The submodules' tests read and
     write their wire formats through it."
    (let [p (port/open-bytes path mode)]
      (defer
        (protect (port/close p))
        (thunk (tcp-transport p)))))

  ## ── Tests ─────────────────────────────────────────────────────────────

  (defn run-tests []
    "Sanity checks on a transport's reads: a line, then bytes by count."
    (with-temp-dir dir
      (let [path (path/join dir "wire")]
        (spit path "status line\r\nbody")
        (with-file-transport path
                             :read (fn [t]
                                     (assert (= "status line" (t-read-line t))
                                       "a line comes back without its CRLF")
                                     (assert (= (bytes "body") (t-exact t 4))
                                       "the bytes after it, by count")
                                     (assert (nil? (t-read-line t))
                                       "nil at the end")))))
    true)

  {:tcp tcp-transport
   :tls tls-transport
   :open open-transport
   :read-line t-read-line
   :exact t-exact
   :write t-write
   :flush t-flush
   :close t-close
   :with-file with-file-transport
   :test run-tests})
