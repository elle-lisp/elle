#!/usr/bin/env elle

# MQTT — packet codec demonstration
#
# Demonstrates:
#   elle-mqtt plugin       — MQTT packet encode/decode (no I/O)
#   Encode various packets — CONNECT, PUBLISH, SUBSCRIBE, PING, DISCONNECT
#   Feed + poll            — parse raw bytes back into structured packets
#
# No broker needed — this exercises the codec in isolation.

(def [ok? plugin] (protect (import-file "target/release/libelle_mqtt.so")))
(when (not ok?)
  (println "SKIP: mqtt plugin not built")
  (exit 0))

(def state-fn          (get plugin :state))
(def encode-connect-fn (get plugin :encode-connect))
(def encode-publish-fn (get plugin :encode-publish))
(def encode-subscribe-fn (get plugin :encode-subscribe))
(def encode-ping-fn    (get plugin :encode-ping))
(def encode-disconnect-fn (get plugin :encode-disconnect))
(def feed-fn           (get plugin :feed))
(def poll-fn           (get plugin :poll))
(def connected?-fn     (get plugin :connected?))
(def keep-alive-fn     (get plugin :keep-alive))

(def st (state-fn {:keep-alive 30}))

# ── Encode packets ────────────────────────────────────────────────

(println "encoding packets:")

(let [[pkt (encode-connect-fn st {:client-id "elle-demo" :clean-session true})]]
  (print "  CONNECT:     ") (print (length pkt)) (println " bytes"))

(let [[pkt (encode-publish-fn st "sensors/temp" "23.5")]]
  (print "  PUBLISH:     ") (print (length pkt)) (println " bytes"))

(let [[pkt (encode-publish-fn st "sensors/temp" "23.5" {:qos 1 :retain true})]]
  (print "  PUBLISH q1:  ") (print (length pkt)) (println " bytes"))

(let [[pkt (encode-subscribe-fn st [["sensors/#" 0] ["alerts/#" 1]])]]
  (print "  SUBSCRIBE:   ") (print (length pkt)) (println " bytes"))

(let [[pkt (encode-ping-fn st)]]
  (print "  PINGREQ:     ") (print (length pkt)) (println " bytes"))

(let [[pkt (encode-disconnect-fn st)]]
  (print "  DISCONNECT:  ") (print (length pkt)) (println " bytes"))

# ── Feed synthetic packets and poll ───────────────────────────────

(println "")
(println "decoding packets:")

# Synthetic CONNACK (success)
(feed-fn st (bytes 32 2 0 0))
(let [[pkt (poll-fn st)]]
  (println "  CONNACK:     code=" pkt:code " session-present=" pkt:session-present))

(println "  connected?   " (connected?-fn st))
(println "  keep-alive:  " (keep-alive-fn st) "s")

# Synthetic PINGRESP
(feed-fn st (bytes 208 0))
(let [[pkt (poll-fn st)]]
  (println "  PINGRESP:    type=" pkt:type))

# Synthetic PUBLISH (topic "t", payload "hi", QoS 0)
(feed-fn st (bytes 48 5 0 1 116 104 105))
(let [[pkt (poll-fn st)]]
  (println "  PUBLISH:     topic=" pkt:topic " payload=" (string pkt:payload) " qos=" pkt:qos))

# Synthetic SUBACK (packet-id 1, code 0)
(feed-fn st (bytes 144 3 0 1 0))
(let [[pkt (poll-fn st)]]
  (println "  SUBACK:      packet-id=" pkt:packet-id " codes=" pkt:codes))

(println "")
(println "all mqtt examples passed.")
