(elle/epoch 15)
# audited: 2026-11-12
## The TCP and TLS transports of HTTP/2, each writing through a gather over its connection.
## lib/http2/overview.md
##
## Loaded via:
##   (def transport ((import "std/http2/transport") :tls tls))
##
## Exports: {:tcp :tls}

(fn [&named tls]
  (def gather ((import "std/gather")))

  ## A transport is {:read :write :flush :close}. A write queues its value in
  ## the gather, and a flush sends the queue in one write (lib/gather.md).

  (defn tcp-transport [port]
    "Wrap a TCP port as a transport. A flush sends the queued frames in one
     gathered write, each part from where it lies."
    (let [g (gather:open port)]
      {:read (fn [n] (port/read port n))
       :write (fn [data] (gather:write g data))
       :flush (fn [] (gather:flush g))
       :close (fn [] (port/close port))}))

  (defn tls-transport [conn]
    "Wrap a TLS connection as a transport. A flush joins the queued frames
     into one tls:write."
    (let [g (gather:open (fn [parts] (tls:write conn (gather:join parts))))]
      {:read (fn [n] (tls:read conn n))
       :write (fn [data] (gather:write g data))
       :flush (fn [] (gather:flush g))
       :close (fn [] (tls:close conn))}))

  {:tcp tcp-transport :tls tls-transport})
