(elle/epoch 12)
## tests/elle/connect-resolve.lisp
## TCP connect is IP-only at the primitive layer (tcp/connect-ip) with hostname
## resolution lifted into the stdlib wrapper (tcp/connect). The wrapper resolves
## the host and tries each returned address in order, so a multi-record host whose
## first address is down (e.g. an IPv6 record with no listener) falls back to a
## later address — the multi-address behaviour the fused libc connect used to give.

(defn tcp-port [listener]
  "Numeric port from a listener's path (e.g. '127.0.0.1:8080')."
  (let [parts (string/split (port/path listener) ":")]
    (parse-int (get parts (- (length parts) 1)))))

(defn echo-once [listener]
  "Accept one connection, echo a chunk back, close."
  (ev/spawn (fn []
              (let [conn (tcp/accept listener :timeout 5000)
                    data (port/read conn 1024)]
                (port/write conn data)
                (port/close conn)))))

## ── tcp/connect-ip: the IP-only primitive ───────────────────────────

## Connects when given a parsed IP literal.
(let [listener (tcp/listen "127.0.0.1" 0)
      port (tcp-port listener)]
  (echo-once listener)
  (let [conn (tcp/connect-ip "127.0.0.1" port :timeout 5000)]
    (port/write conn "ip-only")
    (assert (= (string (port/read conn 1024)) "ip-only")
            "tcp/connect-ip connects by IP literal")
    (port/close conn))
  (port/close listener))

## Rejects a hostname synchronously (parse failure before any yield), so protect
## — which is synchronous — catches it.
(let [[ok? _] (protect (tcp/connect-ip "localhost" 80))]
  (assert (not ok?) "tcp/connect-ip rejects a hostname"))

## Rejects a bad port synchronously, like the old primitive did.
(let [[ok? _] (protect (tcp/connect-ip "127.0.0.1" 99999))]
  (assert (not ok?) "tcp/connect-ip rejects an out-of-range port"))

## ── sys/ip?: the synchronous IP-literal predicate ───────────────────
## tcp/connect branches on this to skip resolution (a pool getaddrinfo
## round-trip plus a scheduler yield) when the host is already an IP
## literal. It is a total predicate: a non-string, or a string that does
## not parse as an IPv4/IPv6 literal, is false — never an error.

(assert (sys/ip? "127.0.0.1") "sys/ip?: IPv4 literal")
(assert (sys/ip? "0.0.0.0") "sys/ip?: all-zeros IPv4")
(assert (sys/ip? "255.255.255.255") "sys/ip?: max IPv4")
(assert (sys/ip? "::1") "sys/ip?: IPv6 loopback")
(assert (sys/ip? "2001:db8::1") "sys/ip?: IPv6 literal")
(assert (not (sys/ip? "localhost")) "sys/ip?: hostname is not an IP")
(assert (not (sys/ip? "example.com")) "sys/ip?: domain is not an IP")
(assert (not (sys/ip? "256.0.0.1")) "sys/ip?: octet > 255 is not an IP")
(assert (not (sys/ip? "127.0.0.1.5")) "sys/ip?: extra octet is not an IP")
(assert (not (sys/ip? "127.0.0.1:80")) "sys/ip?: host:port is not an IP")
(assert (not (sys/ip? "[::1]")) "sys/ip?: bracketed IPv6 is not a bare IP")
(assert (not (sys/ip? "")) "sys/ip?: empty string is not an IP")
(assert (not (sys/ip? 42)) "sys/ip?: integer is not an IP")
(assert (not (sys/ip? nil)) "sys/ip?: nil is not an IP")

## ── tcp/connect: the resolving stdlib wrapper ───────────────────────

## Accepts an IP literal — sys/ip? routes it straight to tcp/connect-ip with
## no resolution (no sys/resolve pool op).
(let [listener (tcp/listen "127.0.0.1" 0)
      port (tcp-port listener)]
  (echo-once listener)
  (let [conn (tcp/connect "127.0.0.1" port :timeout 5000)]
    (port/write conn "wrap-ip")
    (assert (= (string (port/read conn 1024)) "wrap-ip")
            "tcp/connect connects by IP literal")
    (port/close conn))
  (port/close listener))

## Accepts a hostname. "localhost" commonly resolves to [::1 127.0.0.1]; the
## listener binds 127.0.0.1 only, so the IPv6 address (no listener) is refused
## and the wrapper falls back to the IPv4 address. This pins multi-record
## try-each — a single-address (first-only) wrapper would fail here.
(let [listener (tcp/listen "127.0.0.1" 0)
      port (tcp-port listener)]
  (echo-once listener)
  (let [conn (tcp/connect "localhost" port :timeout 5000)]
    (port/write conn "wrap-host")
    (assert (= (string (port/read conn 1024)) "wrap-host")
            "tcp/connect resolves a hostname and tries each address")
    (port/close conn))
  (port/close listener))

## A host that does not resolve (or resolves to addresses that all refuse)
## surfaces an error rather than hanging or returning a broken port. Driven via
## ev/spawn + ev/join-protected because the wrapper is async (it yields in
## sys/resolve), so protect cannot wrap it.
(let [[ok? _] (ev/join-protected (ev/spawn (fn []
                                   (tcp/connect "no-such-host.test.invalid" 80
                                   :timeout 1000))))]
  (assert (not ok?) "tcp/connect to an unreachable host errors"))

(println "connect-resolve: all tests passed")
