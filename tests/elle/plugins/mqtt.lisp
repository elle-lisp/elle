(elle/epoch 8)
## MQTT plugin integration tests
## Tests the mqtt plugin (.so loaded via import-file)

## Try to load the MQTT plugin. If it fails, exit cleanly.
(def [ok? plugin] (protect (import-file "target/release/libelle_mqtt.so")))
(when (not ok?)
  (print "SKIP: mqtt plugin not built\n")
  (exit 0))

## Extract plugin functions from the returned struct
(def state-fn          (get plugin :state))
(def encode-connect-fn (get plugin :encode-connect))
(def encode-publish-fn (get plugin :encode-publish))
(def encode-subscribe-fn (get plugin :encode-subscribe))
(def encode-unsubscribe-fn (get plugin :encode-unsubscribe))
(def encode-ping-fn    (get plugin :encode-ping))
(def encode-disconnect-fn (get plugin :encode-disconnect))
(def encode-puback-fn  (get plugin :encode-puback))
(def feed-fn           (get plugin :feed))
(def poll-fn           (get plugin :poll))
(def poll-all-fn       (get plugin :poll-all))
(def next-packet-id-fn (get plugin :next-packet-id))
(def connected?-fn     (get plugin :connected?))
(def keep-alive-fn     (get plugin :keep-alive))

## ── mqtt/state — default options ──────────────────────────────────

(let [st (state-fn)]
  (assert (= (keep-alive-fn st) 60) "default keep-alive is 60")
  (assert (= (connected?-fn st) false) "not connected initially"))

## ── mqtt/state — custom options ───────────────────────────────────

(let [st (state-fn {:keep-alive 30})]
  (assert (= (keep-alive-fn st) 30) "custom keep-alive"))

## ── mqtt/next-packet-id — monotonic ──────────────────────────────

(let [st (state-fn)]
  (assert (= (next-packet-id-fn st) 1) "first packet id is 1")
  (assert (= (next-packet-id-fn st) 2) "second packet id is 2")
  (assert (= (next-packet-id-fn st) 3) "third packet id is 3"))

## ── mqtt/encode-connect — produces bytes ─────────────────────────

(let* [st (state-fn)
       pkt (encode-connect-fn st {:client-id "test-client" :clean-session true})]
  (assert (> (length pkt) 0) "CONNECT packet is non-empty")
  # MQTT fixed header: first byte 0x10 = CONNECT
  (assert (= (get pkt 0) 16) "CONNECT packet starts with 0x10"))

## ── mqtt/encode-publish — QoS 0 ──────────────────────────────────

(let* [st (state-fn)
       pkt (encode-publish-fn st "test/topic" "hello")]
  (assert (> (length pkt) 0) "PUBLISH packet is non-empty")
  # MQTT fixed header: first byte 0x30 = PUBLISH (QoS 0, no retain)
  (assert (= (get pkt 0) 48) "PUBLISH QoS 0 starts with 0x30"))

## ── mqtt/encode-publish — QoS 1 ──────────────────────────────────

(let* [st (state-fn)
       pkt (encode-publish-fn st "test/topic" "hello" {:qos 1})]
  (assert (> (length pkt) 0) "PUBLISH QoS 1 packet is non-empty")
  # QoS 1 PUBLISH: first byte 0x32 = 0x30 | (1 << 1)
  (assert (= (get pkt 0) 50) "PUBLISH QoS 1 starts with 0x32"))

## ── mqtt/encode-subscribe ────────────────────────────────────────

(let* [st (state-fn)
       pkt (encode-subscribe-fn st [["sensors/#" 0]])]
  (assert (> (length pkt) 0) "SUBSCRIBE packet is non-empty")
  # MQTT fixed header: first byte 0x82 = SUBSCRIBE
  (assert (= (get pkt 0) 130) "SUBSCRIBE starts with 0x82"))

## ── mqtt/encode-unsubscribe ──────────────────────────────────────

(let* [st (state-fn)
       pkt (encode-unsubscribe-fn st ["sensors/#"])]
  (assert (> (length pkt) 0) "UNSUBSCRIBE packet is non-empty")
  # MQTT fixed header: first byte 0xA2 = UNSUBSCRIBE
  (assert (= (get pkt 0) 162) "UNSUBSCRIBE starts with 0xA2"))

## ── mqtt/encode-ping ─────────────────────────────────────────────

(let* [st (state-fn)
       pkt (encode-ping-fn st)]
  (assert (= (length pkt) 2) "PINGREQ is 2 bytes")
  # 0xC0 0x00
  (assert (= (get pkt 0) 192) "PINGREQ first byte 0xC0")
  (assert (= (get pkt 1) 0)   "PINGREQ second byte 0x00"))

## ── mqtt/encode-disconnect ───────────────────────────────────────

(let* [st (state-fn)
       pkt (encode-disconnect-fn st)]
  (assert (= (length pkt) 2) "DISCONNECT is 2 bytes")
  # 0xE0 0x00
  (assert (= (get pkt 0) 224) "DISCONNECT first byte 0xE0")
  (assert (= (get pkt 1) 0)   "DISCONNECT second byte 0x00"))

## ── mqtt/encode-puback ───────────────────────────────────────────

(let* [st (state-fn)
       pkt (encode-puback-fn st 42)]
  (assert (= (length pkt) 4) "PUBACK is 4 bytes")
  # 0x40 = PUBACK
  (assert (= (get pkt 0) 64) "PUBACK first byte 0x40"))

## ── mqtt/feed + mqtt/poll — synthetic CONNACK ────────────────────

(let* [st (state-fn)
       # CONNACK packet: 20 02 00 00
       # byte 0: 0x20 = CONNACK, byte 1: 0x02 = remaining length
       # byte 2: 0x00 = no session present, byte 3: 0x00 = accepted
       connack (bytes 32 2 0 0)
       count (feed-fn st connack)]
  (assert (>= count 1) "feed parsed at least one packet")
  (let [pkt (poll-fn st)]
    (assert (not (nil? pkt)) "poll returns a packet")
    (assert (= pkt:type :connack) "packet type is :connack")
    (assert (= pkt:code 0) "CONNACK code is 0 (success)")
    (assert (= pkt:session-present false) "no session present"))
  (assert (= (connected?-fn st) true) "connected after CONNACK"))

## ── mqtt/feed + mqtt/poll — synthetic PUBLISH ────────────────────

(let* [st (state-fn)
       # Build a PUBLISH packet for topic "t" with payload "hi"
       # 0x30 = PUBLISH QoS 0, remaining len = 2(topic len) + 1(topic) + 2(payload) = 5
       pub-bytes (bytes 48 5 0 1 116 104 105)
       count (feed-fn st pub-bytes)]
  (assert (>= count 1) "feed parsed PUBLISH")
  (let [pkt (poll-fn st)]
    (assert (= pkt:type :publish) "packet type is :publish")
    (assert (= pkt:topic "t") "topic is 't'")
    (assert (= pkt:payload (bytes "hi")) "payload is 'hi'")
    (assert (= pkt:qos 0) "qos is 0")))

## ── mqtt/feed + mqtt/poll — PINGRESP ─────────────────────────────

(let* [st (state-fn)
       pingresp (bytes 208 0)
       count (feed-fn st pingresp)]
  (assert (>= count 1) "feed parsed PINGRESP")
  (let [pkt (poll-fn st)]
    (assert (= pkt:type :pingresp) "packet type is :pingresp")))

## ── mqtt/poll — returns nil when empty ───────────────────────────

(let* [st (state-fn)
       pkt (poll-fn st)]
  (assert (nil? pkt) "poll returns nil when no packets"))

## ── mqtt/poll-all ────────────────────────────────────────────────

(let* [st (state-fn)
       # Feed two packets: CONNACK + PINGRESP
       _ (feed-fn st (bytes 32 2 0 0))
       _ (feed-fn st (bytes 208 0))
       pkts (poll-all-fn st)]
  (assert (= (length pkts) 2) "poll-all returns 2 packets")
  (let [p0 (get pkts 0)
        p1 (get pkts 1)]
    (assert (= p0:type :connack) "first is CONNACK")
    (assert (= p1:type :pingresp) "second is PINGRESP")))

## ── Round-trip: encode then feed+poll ────────────────────────────

(let* [st1 (state-fn)
       st2 (state-fn)
       # Encode a CONNECT from st1
       connect-bytes (encode-connect-fn st1 {:client-id "roundtrip"})
       # Feed it into st2 — st2 acts as "the other side" parsing
       count (feed-fn st2 connect-bytes)]
  (assert (>= count 1) "round-trip: CONNECT parsed"))

(println "all mqtt tests passed.")
