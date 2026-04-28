(elle/epoch 9)
## lib/http2/transport.lisp — transport abstraction for HTTP/2
##
## Loaded via:
##   (def transport ((import "std/http2/transport") :tls tls))
##
## Exports: {:tcp :tls}

(fn [&named tls]
  (defn tcp-transport [port]
    "Wrap a TCP port as a transport with buffered binary writes."
    (def @wbuf-parts @[])
    {:read (fn [n] (port/read port n))
     :write (fn [data]
              (let [d (if (bytes? data) data (bytes data))]
                (push wbuf-parts d)))
     :flush (fn []
              (when (> (length wbuf-parts) 0)
                (let [combined (apply concat (freeze wbuf-parts))]
                  (port/write port combined)
                  (assign wbuf-parts @[]))))
     :close (fn [] (port/close port))})
  (defn tls-transport [conn]
    "Wrap a TLS connection as a transport."
    {:read (fn [n] (tls:read conn n))
     :write (fn [data] (tls:write conn data))
     :flush (fn [] nil)
     :close (fn [] (tls:close conn))})
  {:tcp tcp-transport :tls tls-transport})
