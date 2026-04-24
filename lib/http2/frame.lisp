(elle/epoch 9)
## lib/http2/frame.lisp — HTTP/2 frame codec (RFC 9113)
##
## Loaded via: (def frame ((import "std/http2/frame")))
##
## Exports: {:read-frame :write-frame :make-* :parse-settings :constants :test}

(fn []

  ## ── Constants ──────────────────────────────────────────────────────────

  # Frame types (RFC 9113 Section 6)
  (def TYPE-DATA          0)
  (def TYPE-HEADERS       1)
  (def TYPE-PRIORITY      2)
  (def TYPE-RST-STREAM    3)
  (def TYPE-SETTINGS      4)
  (def TYPE-PUSH-PROMISE  5)
  (def TYPE-PING          6)
  (def TYPE-GOAWAY        7)
  (def TYPE-WINDOW-UPDATE 8)
  (def TYPE-CONTINUATION  9)

  # Flags (combined per frame type)
  (def FLAG-ACK         0x1)
  (def FLAG-END-STREAM  0x1)
  (def FLAG-END-HEADERS 0x4)
  (def FLAG-PADDED      0x8)
  (def FLAG-PRIORITY    0x20)

  # Error codes (RFC 9113 Section 7)
  (def ERR-NO-ERROR            0x0)
  (def ERR-PROTOCOL-ERROR      0x1)
  (def ERR-INTERNAL-ERROR      0x2)
  (def ERR-FLOW-CONTROL-ERROR  0x3)
  (def ERR-SETTINGS-TIMEOUT    0x4)
  (def ERR-STREAM-CLOSED       0x5)
  (def ERR-FRAME-SIZE-ERROR    0x6)
  (def ERR-REFUSED-STREAM      0x7)
  (def ERR-CANCEL              0x8)
  (def ERR-COMPRESSION-ERROR   0x9)
  (def ERR-CONNECT-ERROR       0xa)
  (def ERR-ENHANCE-YOUR-CALM   0xb)
  (def ERR-INADEQUATE-SECURITY 0xc)
  (def ERR-HTTP-1-1-REQUIRED   0xd)

  # Settings identifiers (RFC 9113 Section 6.5.2)
  (def SETTINGS-HEADER-TABLE-SIZE      0x1)
  (def SETTINGS-ENABLE-PUSH            0x2)
  (def SETTINGS-MAX-CONCURRENT-STREAMS 0x3)
  (def SETTINGS-INITIAL-WINDOW-SIZE    0x4)
  (def SETTINGS-MAX-FRAME-SIZE         0x5)
  (def SETTINGS-MAX-HEADER-LIST-SIZE   0x6)

  # Defaults
  (def DEFAULT-HEADER-TABLE-SIZE      4096)
  (def DEFAULT-INITIAL-WINDOW-SIZE    65535)
  (def DEFAULT-MAX-FRAME-SIZE         16384)
  (def DEFAULT-MAX-HEADER-LIST-SIZE   -1)  # unlimited
  (def MAX-WINDOW-SIZE                2147483647)  # 2^31 - 1

  # Connection preface (RFC 9113 Section 3.4)
  (def CLIENT-PREFACE (bytes 0x50 0x52 0x49 0x20 0x2a 0x20 0x48 0x54
                             0x54 0x50 0x2f 0x32 0x2e 0x30 0x0d 0x0a
                             0x0d 0x0a 0x53 0x4d 0x0d 0x0a 0x0d 0x0a))

  ## ── Byte packing helpers ──────────────────────────────────────────────

  (defn u16->bytes [n]
    (bytes (bit/and (bit/shr n 8) 0xff)
           (bit/and n 0xff)))

  (defn u24->bytes [n]
    (bytes (bit/and (bit/shr n 16) 0xff)
           (bit/and (bit/shr n 8) 0xff)
           (bit/and n 0xff)))

  (defn u32->bytes [n]
    (bytes (bit/and (bit/shr n 24) 0xff)
           (bit/and (bit/shr n 16) 0xff)
           (bit/and (bit/shr n 8) 0xff)
           (bit/and n 0xff)))

  (defn read-u16 [buf offset]
    (bit/or (bit/shl (get buf offset) 8)
            (get buf (+ offset 1))))

  (defn read-u24 [buf offset]
    (bit/or (bit/or (bit/shl (get buf offset) 16)
                    (bit/shl (get buf (+ offset 1)) 8))
            (get buf (+ offset 2))))

  (defn read-u32 [buf offset]
    (bit/or
      (bit/or (bit/shl (get buf offset) 24)
              (bit/shl (get buf (+ offset 1)) 16))
      (bit/or (bit/shl (get buf (+ offset 2)) 8)
              (get buf (+ offset 3)))))

  ## ── Frame header codec ────────────────────────────────────────────────
  ## 9-byte header: length(24) + type(8) + flags(8) + R(1) + stream_id(31)

  (defn encode-header [frame-type flags stream-id payload-len]
    "Encode a 9-byte frame header."
    (concat (u24->bytes payload-len)
            (bytes frame-type)
            (bytes flags)
            (u32->bytes (bit/and stream-id 0x7fffffff))))

  (defn decode-header [buf]
    "Decode a 9-byte frame header from buf. Returns {:length :type :flags :stream-id}."
    {:length    (read-u24 buf 0)
     :type      (get buf 3)
     :flags     (get buf 4)
     :stream-id (bit/and (read-u32 buf 5) 0x7fffffff)})

  ## ── Transport read helpers ────────────────────────────────────────────

  (defn read-exact [transport n]
    "Read exactly n bytes from transport. Returns bytes or nil on EOF."
    (def @buf (bytes))
    (def @remaining n)
    (while (> remaining 0)
      (let [chunk (transport:read remaining)]
        (when (nil? chunk)
          (if (= (length buf) 0)
            (break (begin (assign buf nil) nil))
            (error {:error :h2-error :reason :protocol-error
                    :message "unexpected EOF in frame"})))
        (assign buf (concat buf chunk))
        (assign remaining (- remaining (length chunk)))))
    buf)

  ## ── Frame reader ──────────────────────────────────────────────────────

  (defn read-frame [transport max-size]
    "Read a complete frame from transport. Returns {:length :type :flags :stream-id :payload}
     or nil on clean EOF. Signals :h2-error on protocol violations."
    (let [header-bytes (read-exact transport 9)]
      (if (nil? header-bytes)
        nil
        (let* [hdr (decode-header header-bytes)
               len hdr:length]
          (when (> len max-size)
            (error {:error :h2-error :reason :frame-size-error
                    :code ERR-FRAME-SIZE-ERROR
                    :length len :max max-size
                    :message (concat "frame too large: " (string len))}))
          (let [payload (if (= len 0) (bytes) (read-exact transport len))]
            (put hdr :payload payload))))))

  ## ── Frame writer ──────────────────────────────────────────────────────

  (defn write-frame [transport frame-type flags stream-id payload]
    "Write a complete frame to transport. Returns nil."
    (let* [payload-bytes (or payload (bytes))
           header (encode-header frame-type flags stream-id (length payload-bytes))]
      (transport:write header)
      (when (> (length payload-bytes) 0)
        (transport:write payload-bytes))))

  ## ── Frame builders ────────────────────────────────────────────────────

  (defn make-data-frame [stream-id data end-stream?]
    "Build a DATA frame. Returns [type flags stream-id payload]."
    [TYPE-DATA (if end-stream? FLAG-END-STREAM 0) stream-id data])

  (defn make-headers-frame [stream-id header-block end-stream? end-headers?]
    "Build a HEADERS frame. Returns [type flags stream-id payload]."
    (let [flags (bit/or (if end-stream? FLAG-END-STREAM 0)
                        (if end-headers? FLAG-END-HEADERS 0))]
      [TYPE-HEADERS flags stream-id header-block]))

  (defn make-settings-frame [settings]
    "Build a SETTINGS frame from a list of [id value] pairs.
     Returns [type flags stream-id payload]."
    (let [payload (fold (fn [acc pair]
                          (concat acc (u16->bytes (get pair 0))
                                      (u32->bytes (get pair 1))))
                        (bytes)
                        settings)]
      [TYPE-SETTINGS 0 0 payload]))

  (defn make-settings-ack []
    "Build a SETTINGS ACK frame. Returns [type flags stream-id payload]."
    [TYPE-SETTINGS FLAG-ACK 0 (bytes)])

  (defn make-window-update-frame [stream-id increment]
    "Build a WINDOW_UPDATE frame. Returns [type flags stream-id payload]."
    [TYPE-WINDOW-UPDATE 0 stream-id (u32->bytes (bit/and increment 0x7fffffff))])

  (defn make-rst-stream-frame [stream-id error-code]
    "Build a RST_STREAM frame. Returns [type flags stream-id payload]."
    [TYPE-RST-STREAM 0 stream-id (u32->bytes error-code)])

  (defn make-goaway-frame [last-stream-id error-code &named debug-data]
    "Build a GOAWAY frame. Returns [type flags stream-id payload]."
    (let [payload (concat (u32->bytes (bit/and last-stream-id 0x7fffffff))
                          (u32->bytes error-code)
                          (or debug-data (bytes)))]
      [TYPE-GOAWAY 0 0 payload]))

  (defn make-ping-frame [opaque-data &named ack?]
    "Build a PING frame. Returns [type flags stream-id payload].
     opaque-data must be exactly 8 bytes."
    [TYPE-PING (if ack? FLAG-ACK 0) 0 opaque-data])

  ## ── Settings parser ───────────────────────────────────────────────────

  (defn parse-settings [payload]
    "Parse a SETTINGS frame payload into a list of {:id :value} structs."
    (def @result @[])
    (def @offset 0)
    (while (<= (+ offset 5) (length payload))
      (push result {:id    (read-u16 payload offset)
                    :value (read-u32 payload (+ offset 2))})
      (assign offset (+ offset 6)))
    (freeze result))

  ## ── Convenience predicates ────────────────────────────────────────────

  (defn has-flag? [flags flag]
    "Check if a flag bit is set."
    (not (= 0 (bit/and flags flag))))

  ## ── Constants struct ──────────────────────────────────────────────────

  (def constants
    {:type-data          TYPE-DATA
     :type-headers       TYPE-HEADERS
     :type-priority      TYPE-PRIORITY
     :type-rst-stream    TYPE-RST-STREAM
     :type-settings      TYPE-SETTINGS
     :type-push-promise  TYPE-PUSH-PROMISE
     :type-ping          TYPE-PING
     :type-goaway        TYPE-GOAWAY
     :type-window-update TYPE-WINDOW-UPDATE
     :type-continuation  TYPE-CONTINUATION
     :flag-ack           FLAG-ACK
     :flag-end-stream    FLAG-END-STREAM
     :flag-end-headers   FLAG-END-HEADERS
     :flag-padded        FLAG-PADDED
     :flag-priority      FLAG-PRIORITY
     :err-no-error            ERR-NO-ERROR
     :err-protocol-error      ERR-PROTOCOL-ERROR
     :err-internal-error      ERR-INTERNAL-ERROR
     :err-flow-control-error  ERR-FLOW-CONTROL-ERROR
     :err-settings-timeout    ERR-SETTINGS-TIMEOUT
     :err-stream-closed       ERR-STREAM-CLOSED
     :err-frame-size-error    ERR-FRAME-SIZE-ERROR
     :err-refused-stream      ERR-REFUSED-STREAM
     :err-cancel              ERR-CANCEL
     :err-compression-error   ERR-COMPRESSION-ERROR
     :err-connect-error       ERR-CONNECT-ERROR
     :err-enhance-your-calm   ERR-ENHANCE-YOUR-CALM
     :err-inadequate-security ERR-INADEQUATE-SECURITY
     :err-http-1-1-required   ERR-HTTP-1-1-REQUIRED
     :settings-header-table-size      SETTINGS-HEADER-TABLE-SIZE
     :settings-enable-push            SETTINGS-ENABLE-PUSH
     :settings-max-concurrent-streams SETTINGS-MAX-CONCURRENT-STREAMS
     :settings-initial-window-size    SETTINGS-INITIAL-WINDOW-SIZE
     :settings-max-frame-size         SETTINGS-MAX-FRAME-SIZE
     :settings-max-header-list-size   SETTINGS-MAX-HEADER-LIST-SIZE
     :default-header-table-size       DEFAULT-HEADER-TABLE-SIZE
     :default-initial-window-size     DEFAULT-INITIAL-WINDOW-SIZE
     :default-max-frame-size          DEFAULT-MAX-FRAME-SIZE
     :max-window-size                 MAX-WINDOW-SIZE
     :client-preface                  CLIENT-PREFACE})

  ## ── Tests ──────────────────────────────────────────────────────────────

  (defn run-tests []
    # ── Byte helpers ──
    (assert (= (u16->bytes 0)     (bytes 0 0))       "u16->bytes 0")
    (assert (= (u16->bytes 256)   (bytes 1 0))       "u16->bytes 256")
    (assert (= (u16->bytes 0x1234) (bytes 0x12 0x34)) "u16->bytes 0x1234")

    (assert (= (u24->bytes 0)       (bytes 0 0 0))           "u24->bytes 0")
    (assert (= (u24->bytes 0x123456) (bytes 0x12 0x34 0x56)) "u24->bytes 0x123456")

    (assert (= (u32->bytes 0)         (bytes 0 0 0 0))               "u32->bytes 0")
    (assert (= (u32->bytes 0x12345678) (bytes 0x12 0x34 0x56 0x78)) "u32->bytes 0x12345678")

    (assert (= (read-u16 (bytes 0x12 0x34) 0) 0x1234) "read-u16")
    (assert (= (read-u24 (bytes 0x12 0x34 0x56) 0) 0x123456) "read-u24")
    (assert (= (read-u32 (bytes 0x12 0x34 0x56 0x78) 0) 0x12345678) "read-u32")

    # ── Roundtrip ──
    (assert (= (read-u16 (u16->bytes 12345) 0) 12345) "u16 roundtrip")
    (assert (= (read-u24 (u24->bytes 123456) 0) 123456) "u24 roundtrip")
    (assert (= (read-u32 (u32->bytes 1234567890) 0) 1234567890) "u32 roundtrip")

    # ── Frame header encode/decode roundtrip ──
    (let* [hdr (encode-header TYPE-HEADERS 0x5 3 42)
           decoded (decode-header hdr)]
      (assert (= (length hdr) 9) "frame header: 9 bytes")
      (assert (= decoded:length 42) "frame header: length")
      (assert (= decoded:type TYPE-HEADERS) "frame header: type")
      (assert (= decoded:flags 0x5) "frame header: flags")
      (assert (= decoded:stream-id 3) "frame header: stream-id"))

    # ── Stream ID strips reserved bit ──
    (let* [hdr (encode-header TYPE-DATA 0 0x80000001 10)
           decoded (decode-header hdr)]
      (assert (= decoded:stream-id 1) "frame header: reserved bit stripped"))

    # ── Settings frame builder ──
    (let [[ftype flags sid payload]
          (make-settings-frame [[SETTINGS-INITIAL-WINDOW-SIZE 32768]
                                [SETTINGS-MAX-FRAME-SIZE 32768]])]
      (assert (= ftype TYPE-SETTINGS) "settings frame: type")
      (assert (= flags 0) "settings frame: flags")
      (assert (= sid 0) "settings frame: stream 0")
      (assert (= (length payload) 12) "settings frame: 2 settings × 6 bytes")
      (let [parsed (parse-settings payload)]
        (assert (= (length parsed) 2) "parse-settings: 2 entries")
        (assert (= (get (get parsed 0) :id) SETTINGS-INITIAL-WINDOW-SIZE)
          "parse-settings: first id")
        (assert (= (get (get parsed 0) :value) 32768)
          "parse-settings: first value")
        (assert (= (get (get parsed 1) :id) SETTINGS-MAX-FRAME-SIZE)
          "parse-settings: second id")))

    # ── Settings ACK ──
    (let [[ftype flags sid payload] (make-settings-ack)]
      (assert (= ftype TYPE-SETTINGS) "settings ack: type")
      (assert (= flags FLAG-ACK) "settings ack: flags")
      (assert (= (length payload) 0) "settings ack: empty payload"))

    # ── WINDOW_UPDATE frame ──
    (let [[ftype flags sid payload] (make-window-update-frame 1 65535)]
      (assert (= ftype TYPE-WINDOW-UPDATE) "window-update: type")
      (assert (= sid 1) "window-update: stream-id")
      (assert (= (read-u32 payload 0) 65535) "window-update: increment"))

    # ── RST_STREAM frame ──
    (let [[ftype flags sid payload] (make-rst-stream-frame 3 ERR-CANCEL)]
      (assert (= ftype TYPE-RST-STREAM) "rst-stream: type")
      (assert (= sid 3) "rst-stream: stream-id")
      (assert (= (read-u32 payload 0) ERR-CANCEL) "rst-stream: error code"))

    # ── GOAWAY frame ──
    (let [[ftype flags sid payload]
          (make-goaway-frame 5 ERR-NO-ERROR :debug-data (bytes "bye"))]
      (assert (= ftype TYPE-GOAWAY) "goaway: type")
      (assert (= sid 0) "goaway: stream 0")
      (assert (= (bit/and (read-u32 payload 0) 0x7fffffff) 5) "goaway: last-stream-id")
      (assert (= (read-u32 payload 4) ERR-NO-ERROR) "goaway: error code")
      (assert (= (slice payload 8 (length payload)) (bytes "bye")) "goaway: debug data"))

    # ── PING frame ──
    (let [[ftype flags sid payload] (make-ping-frame (bytes 1 2 3 4 5 6 7 8))]
      (assert (= ftype TYPE-PING) "ping: type")
      (assert (= flags 0) "ping: no ack")
      (assert (= (length payload) 8) "ping: 8 bytes"))

    (let [[ftype flags sid payload] (make-ping-frame (bytes 1 2 3 4 5 6 7 8) :ack? true)]
      (assert (= flags FLAG-ACK) "ping ack: flags"))

    # ── has-flag? ──
    (assert (has-flag? 0x5 FLAG-END-STREAM)  "has-flag?: end-stream in 0x5")
    (assert (has-flag? 0x5 FLAG-END-HEADERS) "has-flag?: end-headers in 0x5")
    (assert (not (has-flag? 0x5 FLAG-PADDED)) "has-flag?: not padded in 0x5")

    # ── CLIENT-PREFACE ──
    (assert (= (length CLIENT-PREFACE) 24) "client preface: 24 bytes")
    (assert (= (string CLIENT-PREFACE) "PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n")
      "client preface: magic string")

    # ── In-memory read/write roundtrip ──
    # Build a mock transport from a buffer
    (let* [write-buf @[]
           mock-transport
             {:write (fn [data]
                       (let [b (if (string? data) (bytes data) data)]
                         (push write-buf b)))
              :read  nil}]
      # Write a HEADERS frame
      (write-frame mock-transport TYPE-HEADERS 0x5 1 (bytes 0x82 0x86))
      # Reconstruct what was written
      (let* [written (apply concat (freeze write-buf))
             hdr (decode-header written)
             payload (slice written 9 (+ 9 hdr:length))]
        (assert (= hdr:type TYPE-HEADERS) "roundtrip: type")
        (assert (= hdr:flags 0x5) "roundtrip: flags")
        (assert (= hdr:stream-id 1) "roundtrip: stream-id")
        (assert (= hdr:length 2) "roundtrip: payload length")
        (assert (= payload (bytes 0x82 0x86)) "roundtrip: payload")))

    true)

  ## ── Exports ────────────────────────────────────────────────────────────

  {:read-frame      read-frame
   :write-frame     write-frame
   :encode-header   encode-header
   :decode-header   decode-header
   :read-exact      read-exact
   :make-data-frame          make-data-frame
   :make-headers-frame       make-headers-frame
   :make-settings-frame      make-settings-frame
   :make-settings-ack        make-settings-ack
   :make-window-update-frame make-window-update-frame
   :make-rst-stream-frame    make-rst-stream-frame
   :make-goaway-frame        make-goaway-frame
   :make-ping-frame          make-ping-frame
   :parse-settings  parse-settings
   :has-flag?       has-flag?
   :u16->bytes      u16->bytes
   :u24->bytes      u24->bytes
   :u32->bytes      u32->bytes
   :read-u16        read-u16
   :read-u24        read-u24
   :read-u32        read-u32
   :constants       constants
   :test            run-tests})
