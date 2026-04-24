(elle/epoch 9)
## lib/zmq.lisp — ZeroMQ bindings for Elle via FFI
##
## Pure FFI bindings to libzmq. No Rust plugin needed.
##
## Dependencies:
##   - libzmq.so installed on the system
##   - ffi primitives (ffi/native, ffi/defbind, ffi/malloc, etc.)
##
## Usage:
##   (def zmq (import-file "lib/zmq.lisp"))
##   (def ctx (zmq:context))
##   (def sock (zmq:socket ctx :req))
##   (zmq:connect sock "tcp://localhost:5555")
##   (zmq:send sock "hello")
##   (def reply (zmq:recv-string sock))
##   (zmq:close sock)
##   (zmq:term ctx)

# ── Load libzmq ────────────────────────────────────────────────────────

(def libzmq (ffi/native "libzmq.so"))

# ── Constants ──────────────────────────────────────────────────────────

(def ZMQ_PAIR 0) (def ZMQ_PUB  1) (def ZMQ_SUB    2)
(def ZMQ_REQ  3) (def ZMQ_REP  4) (def ZMQ_DEALER 5)
(def ZMQ_ROUTER 6) (def ZMQ_PULL 7) (def ZMQ_PUSH 8)

(def ZMQ_SUBSCRIBE 6) (def ZMQ_UNSUBSCRIBE 7) (def ZMQ_RCVMORE 13)
(def ZMQ_LINGER 17) (def ZMQ_SNDHWM 23) (def ZMQ_RCVHWM 24)
(def ZMQ_RCVTIMEO 27) (def ZMQ_SNDTIMEO 28) (def ZMQ_ROUTING_ID 5)

(def ZMQ_DONTWAIT 1) (def ZMQ_SNDMORE 2)

# zmq_msg_t is a 64-byte opaque struct aligned to pointer size
(def msg-type (ffi/struct @[:i64 :i64 :i64 :i64 :i64 :i64 :i64 :i64]))

(def socket-type-map
  {:pair ZMQ_PAIR :pub ZMQ_PUB :sub ZMQ_SUB :req ZMQ_REQ :rep ZMQ_REP
   :dealer ZMQ_DEALER :router ZMQ_ROUTER :pull ZMQ_PULL :push ZMQ_PUSH})

(def option-map
  {:subscribe ZMQ_SUBSCRIBE :unsubscribe ZMQ_UNSUBSCRIBE :rcvmore ZMQ_RCVMORE
   :linger ZMQ_LINGER :sndhwm ZMQ_SNDHWM :rcvhwm ZMQ_RCVHWM
   :rcvtimeo ZMQ_RCVTIMEO :sndtimeo ZMQ_SNDTIMEO :identity ZMQ_ROUTING_ID})

# Byte-valued socket options (everything else is int-valued)
(def byte-options |:identity|)

# ── Raw C bindings ─────────────────────────────────────────────────────

(ffi/defbind zmq-ctx-new     libzmq "zmq_ctx_new"      :ptr  @[])
(ffi/defbind zmq-ctx-term    libzmq "zmq_ctx_term"     :int  @[:ptr])
(ffi/defbind zmq-socket      libzmq "zmq_socket"       :ptr  @[:ptr :int])
(ffi/defbind zmq-close       libzmq "zmq_close"        :int  @[:ptr])
(ffi/defbind zmq-bind        libzmq "zmq_bind"         :int  @[:ptr :string])
(ffi/defbind zmq-connect     libzmq "zmq_connect"      :int  @[:ptr :string])
(ffi/defbind zmq-unbind      libzmq "zmq_unbind"       :int  @[:ptr :string])
(ffi/defbind zmq-disconnect  libzmq "zmq_disconnect"   :int  @[:ptr :string])
(ffi/defbind zmq-setsockopt  libzmq "zmq_setsockopt"   :int  @[:ptr :int :ptr :size])
(ffi/defbind zmq-getsockopt  libzmq "zmq_getsockopt"   :int  @[:ptr :int :ptr :ptr])
(ffi/defbind zmq-errno       libzmq "zmq_errno"        :int  @[])
(ffi/defbind zmq-strerror    libzmq "zmq_strerror"     :ptr  @[:int])

# zmq_send takes (ptr, len) — use ffi/pin to convert bytes to a pointer.
(ffi/defbind zmq-send        libzmq "zmq_send"         :int  @[:ptr :ptr :size :int])

# zmq_msg_* manages a 64-byte opaque struct via ffi/with-stack.
(ffi/defbind zmq-msg-init    libzmq "zmq_msg_init"     :int  @[:ptr])
(ffi/defbind zmq-msg-close   libzmq "zmq_msg_close"    :int  @[:ptr])
(ffi/defbind zmq-msg-data    libzmq "zmq_msg_data"     :ptr  @[:ptr])
(ffi/defbind zmq-msg-size    libzmq "zmq_msg_size"     :size @[:ptr])
(ffi/defbind zmq-msg-recv    libzmq "zmq_msg_recv"     :int  @[:ptr :ptr :int])

# ── FFI helpers ───────────────────────────────────────────────────────

(defn zmq-error [name]
  "Get last ZMQ error and raise it."
  (error {:error :zmq-error
          :message (concat name ": " (ffi/string (zmq-strerror (zmq-errno))))}))

(defn check [rc name]
  "Check a ZMQ return code; error if negative."
  (when (< rc 0) (zmq-error name))
  rc)

(defn null? [ptr]
  (= (ptr/to-int ptr) 0))

(defn as-bytes [data]
  "Coerce string or bytes to bytes."
  (if (string? data) (bytes data) data))

(defn ptr->bytes [ptr len]
  "Read len bytes from an ffi pointer into a bytes value."
  (if (= len 0) (bytes) (ffi/read ptr (ffi/array :u8 len))))

(defn setsockopt-bytes [sock opt-int buf name]
  (let* [ptr (ffi/pin buf)]
    (defer (ffi/free ptr)
      (check (zmq-setsockopt sock opt-int ptr (length buf)) name))
    nil))

(defn setsockopt-int [sock opt-int value name]
  (ffi/with-stack [[ptr :int value]]
    (check (zmq-setsockopt sock opt-int ptr (ffi/size :int)) name)
    nil))

(defn getsockopt-bytes [sock opt-int name]
  (ffi/with-stack [[szptr :size 256] [buf 256]]
    (let* [rc (zmq-getsockopt sock opt-int buf szptr)
           result (ptr->bytes buf (ffi/read szptr :size))]
      (check rc name)
      result)))

(defn getsockopt-int [sock opt-int name]
  (ffi/with-stack [[buf :int 0] [szptr :size (ffi/size :int)]]
    (let* [rc (zmq-getsockopt sock opt-int buf szptr)
           result (ffi/read buf :int)]
      (check rc name)
      result)))

(defn resolve-option [opt-kw name]
  (let [opt-int (get option-map opt-kw)]
    (when (nil? opt-int)
      (error {:error :zmq-error
              :message (concat name ": unknown option " (string opt-kw))}))
    opt-int))

# ── Public API ─────────────────────────────────────────────────────────

(defn zmq/context []
  "Create a new ZMQ context."
  (let [ctx (zmq-ctx-new)]
    (when (null? ctx) (zmq-error "zmq/context"))
    ctx))

(defn zmq/term [ctx]
  "Terminate a ZMQ context."
  (check (zmq-ctx-term ctx) "zmq/term") nil)

(defn zmq/socket [ctx type-kw]
  "Create a socket. type-kw: :req :rep :pub :sub :push :pull :dealer :router :pair"
  (let [type-int (get socket-type-map type-kw)]
    (unless type-int
      (error {:error :zmq-error
              :message (concat "zmq/socket: unknown type " (string type-kw))}))
    (let [sock (zmq-socket ctx type-int)]
      (when (null? sock) (zmq-error "zmq/socket"))
      sock)))

(defn zmq/close [sock]      (check (zmq-close sock) "zmq/close") nil)
(defn zmq/bind [sock ep]    (check (zmq-bind sock ep) "zmq/bind") nil)
(defn zmq/connect [sock ep] (check (zmq-connect sock ep) "zmq/connect") nil)
(defn zmq/unbind [sock ep]  (check (zmq-unbind sock ep) "zmq/unbind") nil)
(defn zmq/disconnect [sock ep] (check (zmq-disconnect sock ep) "zmq/disconnect") nil)

(defn zmq/send [sock data &named dontwait sndmore]
  "Send bytes or string. :dontwait true for non-blocking, :sndmore true for multipart."
  (let* [buf (as-bytes data)
         flags (+ (if dontwait ZMQ_DONTWAIT 0)
                   (if sndmore  ZMQ_SNDMORE  0))
         ptr (ffi/pin buf)]
    (defer (ffi/free ptr)
      (check (zmq-send sock ptr (length buf) flags) "zmq/send"))
    nil))

(defn zmq/recv [sock &named dontwait]
  "Receive bytes. :dontwait true for non-blocking."
  (let [flags (if dontwait ZMQ_DONTWAIT 0)]
    (ffi/with-stack [[msg (ffi/size msg-type)]]
      (zmq-msg-init msg)
      (let [rc (zmq-msg-recv msg sock flags)]
        (when (< rc 0)
          (zmq-msg-close msg)
          (zmq-error "zmq/recv"))
        (let [result (ptr->bytes (zmq-msg-data msg) (zmq-msg-size msg))]
          (zmq-msg-close msg)
          result)))))

(defn zmq/recv-string [sock &named dontwait]
  "Receive as UTF-8 string. :dontwait true for non-blocking."
  (string (zmq/recv sock :dontwait dontwait)))

(defn zmq/subscribe [sock prefix]
  "Subscribe a SUB socket to a prefix."
  (setsockopt-bytes sock ZMQ_SUBSCRIBE (as-bytes prefix) "zmq/subscribe"))

(defn zmq/unsubscribe [sock prefix]
  "Unsubscribe a SUB socket from a prefix."
  (setsockopt-bytes sock ZMQ_UNSUBSCRIBE (as-bytes prefix) "zmq/unsubscribe"))

(defn zmq/set-option [sock opt-kw value]
  "Set a socket option. opt-kw: :linger :sndhwm :rcvhwm :rcvtimeo :sndtimeo :identity"
  (let [opt-int (resolve-option opt-kw "zmq/set-option")]
    (if (contains? byte-options opt-kw)
      (setsockopt-bytes sock opt-int (as-bytes value) "zmq/set-option")
      (setsockopt-int sock opt-int value "zmq/set-option"))))

(defn zmq/get-option [sock opt-kw]
  "Get a socket option."
  (let [opt-int (resolve-option opt-kw "zmq/get-option")]
    (if (contains? byte-options opt-kw)
      (getsockopt-bytes sock opt-int "zmq/get-option")
      (let [result (getsockopt-int sock opt-int "zmq/get-option")]
        (if (= opt-kw :rcvmore) (not (zero? result)) result)))))

(defn zmq/has-more? [sock]
  "Check if more multipart frames are available."
  (zmq/get-option sock :rcvmore))

(defn zmq/send-multipart [sock frames]
  "Send an array of frames as a multipart message."
  (let [last-idx (- (length frames) 1)]
    (each i in (range (length frames))
      (zmq/send sock (get frames i)
        :sndmore (< i last-idx)))))

(defn zmq/recv-multipart [sock &named dontwait]
  "Receive all frames of a multipart message as an array."
  (let [parts @[]]
    (push parts (zmq/recv sock :dontwait dontwait))
    (while (zmq/has-more? sock)
      (push parts (zmq/recv sock :dontwait dontwait)))
    (freeze parts)))

# ── Export ─────────────────────────────────────────────────────────────

{:context        zmq/context
 :term           zmq/term
 :socket         zmq/socket
 :close          zmq/close
 :bind           zmq/bind
 :connect        zmq/connect
 :unbind         zmq/unbind
 :disconnect     zmq/disconnect
 :send           zmq/send
 :recv           zmq/recv
 :recv-string    zmq/recv-string
 :subscribe      zmq/subscribe
 :unsubscribe    zmq/unsubscribe
 :set-option     zmq/set-option
 :get-option     zmq/get-option
 :has-more?      zmq/has-more?
 :send-multipart zmq/send-multipart
 :recv-multipart zmq/recv-multipart}
