(elle/epoch 9)
## lib/mqtt.lisp — MQTT client for Elle
##
## MQTT client using the elle-mqtt plugin for packet encode/decode.
## All TCP I/O is async via native TCP ports and the fiber scheduler.
##
## Dependencies:
##   - elle-mqtt plugin loaded via (import-file "path/to/libelle_mqtt.so")
##   - tcp/connect, port/read, port/write, port/close
##
## Usage:
##   (def mqtt-plugin (import-file "target/release/libelle_mqtt.so"))
##   (def mqtt ((import-file "lib/mqtt.lisp") mqtt-plugin))
##   (let* [[conn (mqtt:connect "broker.example.com" 1883
##                              :client-id "elle-client")]
##          [_ (mqtt:subscribe conn [["test/#" 0]])]
##          [msg (mqtt:recv conn)]]
##     (println "got:" msg)
##     (mqtt:close conn))
##
## mqtt-conn shape:
##   {:tcp port :mqtt mqtt-state}

## ── Entry-point thunk ─────────────────────────────────────────────────

(fn [plugin]

  ## ── Private helpers ───────────────────────────────────────────────

  (defn drive-until [conn pred]
    "Feed and poll until pred matches a packet. Returns the matching packet.
     Non-matching packets are discarded."
    (forever
      (if-let [pkt (plugin:poll conn:mqtt)]
        (when (pred pkt) (break pkt))
        (let [data (port/read conn:tcp 16384)]
          (when (nil? data)
            (error {:error :mqtt-error :reason :connection-closed :message "connection closed unexpectedly"}))
          (plugin:feed conn:mqtt data)))))

  (defn packet-type? [type-kw]
    "Return a predicate that matches packets of the given type."
    (fn [pkt] (= pkt:type type-kw)))

  (defn send-packet [conn packet-bytes]
    "Write encoded packet bytes to the connection's TCP port."
    (when (nonempty? packet-bytes)
      (port/write conn:tcp packet-bytes)))

  ## ── Public API ────────────────────────────────────────────────────

  (defn mqtt/connect [host port-num &named client-id username password @clean-session keep-alive]
    "Connect to an MQTT broker. Returns {:tcp port :mqtt mqtt-state}."
    (default clean-session true)
    (let* [opts {:client-id client-id :username username :password password
                  :clean-session clean-session :keep-alive keep-alive}
           tcp-port (tcp/connect host port-num)
           mqtt (plugin:state opts)
           conn {:tcp tcp-port :mqtt mqtt}]
      (let [[ok? err] (protect
                         (send-packet conn (plugin:encode-connect mqtt opts))
                         (let [ack (drive-until conn (packet-type? :connack))]
                           (unless (zero? ack:code)
                             (error {:error :mqtt-error
                                     :reason :connack-rejected :code ack:code
                                     :message (concat "CONNACK rejected, code="
                                                      (string ack:code))}))))]
        (unless ok?
          (port/close tcp-port)
          (error err)))
      conn))

  (defn mqtt/publish [conn topic payload &named @qos retain]
    "Publish a message. :qos 0/1/2 (default 0), :retain true/false."
    (default qos 0)
    (let [opts {:qos qos :retain retain}]
      (send-packet conn (plugin:encode-publish conn:mqtt topic payload opts))
      (when (>= qos 1)
        (drive-until conn (packet-type? :puback)))))

  (defn mqtt/subscribe [conn topics]
    "Subscribe to topics. topics: [[\"topic\" 0] ...].
     Returns the SUBACK packet."
    (send-packet conn (plugin:encode-subscribe conn:mqtt topics))
    (drive-until conn (packet-type? :suback)))

  (defn mqtt/unsubscribe [conn topics]
    "Unsubscribe from topics. topics: [\"topic\" ...].
     Returns the UNSUBACK packet."
    (send-packet conn (plugin:encode-unsubscribe conn:mqtt topics))
    (drive-until conn (packet-type? :unsuback)))

  (defn mqtt/recv [conn]
    "Receive one MQTT message (typically a PUBLISH).
     Blocks until a packet is available. Returns the packet struct, or nil on EOF."
    (if-let [pkt (plugin:poll conn:mqtt)]
      pkt
      (forever
        (let [data (port/read conn:tcp 16384)]
          (when (nil? data) (break))
          (plugin:feed conn:mqtt data)
          (when-let [pkt (plugin:poll conn:mqtt)]
            (break pkt))))))

  (defn mqtt/listen [conn callback]
    "Loop receiving messages, calling (callback msg) for each.
     Runs until the connection is closed (port/read returns nil)."
    (forever
      (if-let [pkt (mqtt/recv conn)]
        (callback pkt)
        (break))))

  (defn mqtt/close [conn]
    "Send DISCONNECT and close the connection."
    (send-packet conn (plugin:encode-disconnect conn:mqtt))
    (port/close conn:tcp))

  ## ── Export struct ──────────────────────────────────────────────────
  {:connect     mqtt/connect
   :publish     mqtt/publish
   :subscribe   mqtt/subscribe
   :unsubscribe mqtt/unsubscribe
   :recv        mqtt/recv
   :listen      mqtt/listen
   :close       mqtt/close})
