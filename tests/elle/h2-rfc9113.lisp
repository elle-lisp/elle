(elle/epoch 12)
# What the h2 server does with a frame RFC 9113 forbids.
#
# A connection-level violation must end the connection, and it must end
# it with the code the RFC names. Answering with anything else — a
# stream reset, a silent drop, a different code — leaves the peer to
# guess, and a peer that guesses keeps sending on a connection the
# server has stopped honoring.
#
# Each case below drives a raw transport rather than the client library,
# because the client library will not emit these frames. It performs the
# handshake, writes one violating frame, and reads until GOAWAY. The
# assertion is on the error code the GOAWAY carries.
#
# The violations, and the code each owes (RFC 9113 §6):
#
# | Frame | Violation | Code |
# |---|---|---|
# | DATA | on stream 0 | PROTOCOL_ERROR |
# | HEADERS | on stream 0 | PROTOCOL_ERROR |
# | RST_STREAM | on stream 0 | PROTOCOL_ERROR |
# | SETTINGS | on a non-zero stream | PROTOCOL_ERROR |
# | PING | on a non-zero stream | PROTOCOL_ERROR |
# | PING | payload not 8 bytes | FRAME_SIZE_ERROR |
# | CONTINUATION | with no HEADERS before it | PROTOCOL_ERROR |
# | DATA | between HEADERS and its CONTINUATION | PROTOCOL_ERROR |
#
# See lib/http2/frame.lisp and lib/http2/session.lisp.

(def http2 ((import "std/http2")))
(def h2-frame ((import "std/http2/frame")))
(def h2-transport ((import "std/http2/transport")))
(def FC h2-frame:constants)

# The read size every frame read here uses: one h2 frame never exceeds
# the default SETTINGS_MAX_FRAME_SIZE.
(def max-frame 16384)

# How many frames to read before giving up on a GOAWAY. The server sends
# SETTINGS and WINDOW_UPDATE of its own, so the GOAWAY is not the first
# frame back.
(def goaway-search 20)

(defn listen-ephemeral []
  "A listening socket on a kernel-chosen port, with that port."
  (let* [l (tcp/listen "127.0.0.1" 0)
         p (port/path l)
         port (parse-int (slice p (+ 1 (string/find p ":"))))]
    [l port]))

(defn handshake [t]
  "Send the client preface and SETTINGS, read the server's, and ACK."
  (t:write FC:client-preface)
  (let [[ft fl si pl] (h2-frame:make-settings-frame [[FC:settings-initial-window-size
        65535]])]
    (h2-frame:write-frame t ft fl si pl))
  (t:flush)
  (h2-frame:read-frame t max-frame)  # server SETTINGS
  (h2-frame:read-frame t max-frame)  # server SETTINGS ACK
  (let [[ft fl si pl] (h2-frame:make-settings-ack)]
    (h2-frame:write-frame t ft fl si pl))
  (t:flush))

(defn read-goaway [t]
  "The error code of the first GOAWAY, or nil if the connection ends
   without one."
  (let [@code nil]
    (each _ in (range 0 goaway-search)
      (let [[ok? f] (protect (h2-frame:read-frame t max-frame))]
        (when (or (not ok?) (nil? f)) (break nil))
        (when (= f:type FC:type-goaway)
          (assign code (h2-frame:read-u32 f:payload 4))
          (break nil))))
    code))

(defn goaway-code [lport violate]
  "Open a connection, hand `violate` the handshaken transport, and return
   the GOAWAY code the server answers with."
  (let* [tcp (tcp/connect "127.0.0.1" lport)
         t (h2-transport:tcp tcp)]
    (defer
      (protect (t:close))
      (handshake t)
      (violate t)
      (t:flush)
      (read-goaway t))))

# ── The violations ───────────────────────────────────────────────────

(defn write-made [t made]
  "Write a frame from the [type flags stream payload] a maker returned."
  (let [[ft fl si pl] made]
    (h2-frame:write-frame t ft fl si pl)))

(def cases
  [["DATA on stream 0"
    (fn [t] (write-made t (h2-frame:make-data-frame 0 (bytes "bad") false)))
    FC:err-protocol-error]

   ["HEADERS on stream 0"
    (fn [t]
      (write-made t (h2-frame:make-headers-frame 0 (bytes 0x82) false true)))
    FC:err-protocol-error]

   ["RST_STREAM on stream 0"
    (fn [t] (write-made t (h2-frame:make-rst-stream-frame 0 FC:err-cancel)))
    FC:err-protocol-error]

   ["SETTINGS on stream 1"
    (fn [t]
      (h2-frame:write-frame t FC:type-settings 0 1
                            (concat (h2-frame:u16->bytes FC:settings-initial-window-size)
                                    (h2-frame:u32->bytes 65535))))
    FC:err-protocol-error]

   ["PING on stream 1"
    (fn [t] (h2-frame:write-frame t FC:type-ping 0 1 (bytes 1 2 3 4 5 6 7 8)))
    FC:err-protocol-error]

   ["PING with a 4-byte payload"
    (fn [t] (h2-frame:write-frame t FC:type-ping 0 0 (bytes 1 2 3 4)))
    FC:err-frame-size-error]

   ["CONTINUATION with no HEADERS before it"
    (fn [t]
      (write-made t (h2-frame:make-continuation-frame 1 (bytes 0x82) true)))
    FC:err-protocol-error]

   ["DATA between HEADERS and its CONTINUATION"
    (fn [t]
      # HEADERS without END_HEADERS opens the continuation sequence; only
      # a CONTINUATION may follow it.
      (write-made t (h2-frame:make-headers-frame 1 (bytes 0x82) false false))
      (write-made t (h2-frame:make-data-frame 1 (bytes "bad") false)))
    FC:err-protocol-error]])

# ── Run ──────────────────────────────────────────────────────────────

(println "malformed frames end the connection with the code the RFC names...")

(let* [[listener lport] (listen-ephemeral)
       handler (fn [req] {:status 200 :body "ok"})
       sf (ev/spawn (fn [] (protect (http2:serve listener handler))))]
  (defer
    (begin
      (protect (port/close listener))
      (protect (ev/abort sf)))
    (each [name violate expected] in cases
      (let [code (goaway-code lport violate)]
        (assert (= code expected)
                (string name ": expected GOAWAY " (string expected) ", got "
                        (string code)))
        (println "  " name)))))

(println "h2 rfc 9113: every violation ended the connection with its own code")
