(elle/epoch 15)
# audited: 2026-11-12
## The TCP and TLS transports of HTTP/2, each writing through a gather from std/framing over its connection.
## lib/http2/overview.md
##
## Loaded via:
##   (def transport ((import "std/http2/transport") :tls tls))
##
## Exports: {:tcp :tls}

(fn [&named tls]
  (def framing ((import "std/framing")))

  ## A transport is {:read :write :flush :close}. A write queues its value in
  ## the gather, and a flush sends the queue in one write (lib/framing.md).

  (defn tcp-transport [port]
    "Wrap a TCP port as a transport. A flush sends the queued frames in one
     gathered write, each part from where it lies."
    (let [g (framing:gather port)]
      {:read (fn [n] (port/read port n))
       :write (fn [data] (framing:write g data))
       :flush (fn [] (framing:flush g))
       :close (fn [] (port/close port))}))

  (defn tls-transport [conn]
    "Wrap a TLS connection as a transport. A flush joins the queued frames
     into one tls:write."
    (let [g (framing:gather (fn [parts] (tls:write conn (framing:join parts))))]
      {:read (fn [n] (tls:read conn n))
       :write (fn [data] (framing:write g data))
       :flush (fn [] (framing:flush g))
       :close (fn [] (tls:close conn))}))

  {:tcp tcp-transport :tls tls-transport})
