(elle/epoch 12)
# Ports — lifecycle, predicates, display, and standard port parameters

# Scratch dir for every file-port fixture below; removed at the bottom of the
# file (individual cleanup lines are optional under it).
(def scratch (file/mktempdir))

# === Type predicate ===

(assert (port? (port/stdin)) "port? on stdin")
(assert (port? (port/stdout)) "port? on stdout")
(assert (port? (port/stderr)) "port? on stderr")
(assert (not (port? 42)) "port? on int")
(assert (not (port? "hello")) "port? on string")
(assert (not (port? nil)) "port? on nil")

# === Open predicate ===

(assert (port/open? (port/stdin)) "port/open? on fresh stdin")
(assert (port/open? (port/stdout)) "port/open? on fresh stdout")

# === Close and open? ===

(let [p (port/stdin)]
  (assert (port/open? p) "port open before close")
  (port/close p)
  (assert (not (port/open? p)) "port closed after close"))

# === Idempotent close ===

(let [p (port/stdout)]
  (port/close p)
  (port/close p)
  (assert (not (port/open? p)) "double close is idempotent"))

# === with macro for resource management ===

(with p (port/open (path/join scratch "with") :write) port/close
      (assert (port/open? p) "port open inside with"))

# === File port open/close lifecycle ===

(let [p (port/open (path/join scratch "lifecycle") :write)]
  (assert (port? p) "file port is a port")
  (assert (port/open? p) "file port is open after open")
  (port/close p)
  (assert (not (port/open? p)) "file port is closed after close"))

# === port/open-bytes ===

(let [p (port/open-bytes (path/join scratch "bytes") :write)]
  (assert (port? p) "bytes port is a port")
  (port/close p))

# === Error cases ===

# port/open on nonexistent path — I/O error propagates through protect.
(let [[ok? _] (protect (port/open (path/join scratch "no-such-dir" "file") :read))]
  (assert (not ok?) "port/open on nonexistent path errors"))

(let [[ok? _] (protect ((fn ()
                          (port/open (path/join scratch "badmode") :badmode))))]
  (assert (not ok?) "port/open with bad mode errors"))

(let [[ok? _] (protect ((fn () (port/close 42))))]
  (assert (not ok?) "port/close on non-port errors"))

(let [[ok? _] (protect ((fn () (port/open? 42))))]
  (assert (not ok?) "port/open? on non-port errors"))

# === Display format ===

(assert (= (string (port/stdin)) "#<port:stdin>") "stdin display")
(assert (= (string (port/stdout)) "#<port:stdout>") "stdout display")
(assert (= (string (port/stderr)) "#<port:stderr>") "stderr display")

# === Standard port parameters ===

(assert (parameter? *stdin*) "*stdin* is a parameter")
(assert (parameter? *stdout*) "*stdout* is a parameter")
(assert (parameter? *stderr*) "*stderr* is a parameter")

(assert (port? (*stdin*)) "*stdin* default is a port")
(assert (port? (*stdout*)) "*stdout* default is a port")
(assert (port? (*stderr*)) "*stderr* default is a port")

# === Parameterize standard ports ===

(let [custom-port (port/open (path/join scratch "param") :write)]
  (parameterize ((*stdout* custom-port))
    (assert (port? (*stdout*)) "parameterized *stdout* is a port")
    (assert (port/open? (*stdout*)) "parameterized *stdout* is open"))
  (port/close custom-port))

# === Additional error cases ===

(let [[ok? _] (protect ((fn () (port/open 42 :read))))]
  (assert (not ok?) "port/open with non-string path errors"))

# === Read-write and append modes ===

(let [p (port/open (path/join scratch "readwrite") :read-write)]
  (assert (port? p) "read-write port is a port")
  (assert (port/open? p) "read-write port is open")
  (port/close p))

(let [p (port/open (path/join scratch "append") :append)]
  (assert (port? p) "append port is a port")
  (assert (port/open? p) "append port is open")
  (port/close p))

# === :timeout keyword argument ===

# port/open with :timeout on a regular file completes before the timeout expires.
(let [p (port/open (path/join scratch "timeout") :write :timeout 5000)]
  (assert (port/open? p) "port/open with :timeout works on regular file")
  (port/close p))

# port/open-bytes with :timeout on a regular file completes before the timeout expires.
(let [p (port/open-bytes (path/join scratch "bytes-timeout") :write
                         :timeout 5000)]
  (assert (port/open? p) "port/open-bytes with :timeout works on regular file")
  (port/close p))

# ============================================================================
# Display and type tests (from integration/ports.rs)
# ============================================================================

# port_open_file_display
(let [p (port/open (path/join scratch "display") :write)]
  (let [s (string p)]
    (assert (string-starts-with? s "#<port:file")
            "file port display starts with #<port:file")
    (assert (string-contains? s ":write") "file port display contains :write")
    (assert (string-contains? s ":text") "file port display contains :text"))
  (port/close p))

# port_open_bytes_display
(let [p (port/open-bytes (path/join scratch "bytes-display") :write)]
  (let [s (string p)]
    (assert (string-starts-with? s "#<port:file")
            "bytes port display starts with #<port:file")
    (assert (string-contains? s ":binary") "bytes port display contains :binary"))
  (port/close p))

# port_type_of
(assert (= (type (port/stdin)) :port) "type-of port is :port")

# ==============================
# Seek and Tell on file ports
# ==============================

(def seek-test-path (path/join scratch "seek-tell"))

# --- Basic seek/tell lifecycle ---

(let [p (port/open seek-test-path :read-write)]
  (port/write p "0123456789")  # Seek to start
  (assert (= (port/seek p 0 :from :start) 0) "seek to start returns 0")
  (assert (= (port/tell p) 0) "tell at start returns 0")

  # Seek to position 5
  (assert (= (port/seek p 5 :from :start) 5) "seek to 5 returns 5")
  (assert (= (port/tell p) 5) "tell at 5 returns 5")

  # Seek from end: 0 from end = past last byte = 10
  (assert (= (port/seek p 0 :from :end) 10)
          "seek 0 from end of 10-byte file returns 10")

  # Seek -2 from end = position 8
  (assert (= (port/seek p -2 :from :end) 8) "seek -2 from end returns 8")

  # Seek relative to current (now at 8, go +1 = 9)
  (assert (= (port/seek p 1 :from :current) 9) "seek +1 from current returns 9")

  # Seek default (no :from) = SEEK_SET
  (assert (= (port/seek p 3) 3) "seek with default :from returns 3")

  (port/close p))

# --- Seek + read coherence ---

(let [p (port/open seek-test-path :read-write)]
  (port/write p "hello")
  (port/seek p 0 :from :start)
  (assert (= (port/read p 5) "hello")
          "read after seek to start returns written data")
  (port/close p))

# --- Seek clears buffered data ---
# Re-establish known content before testing buffer clear behavior

(spit seek-test-path "0123456789")
(let [p (port/open seek-test-path :read)]
  (port/read p 1)  # Seek back to 0 must discard buffer so next read starts from byte 0
  (port/seek p 0 :from :start)
  (assert (= (port/read p 1) "0") "first char after seek to 0 is '0'")
  (port/close p))

# --- Error cases ---

# port/stdin and port/stdout are synchronous — no SIG_IO inside the thunk
(let [[ok? _] (protect ((fn () (port/seek (port/stdin) 0))))]
  (assert (not ok?) "port/seek on stdin returns error"))

(let [[ok? _] (protect ((fn () (port/tell (port/stdout)))))]
  (assert (not ok?) "port/tell on stdout returns error"))

# port/open inside assert-err yields SIG_IO which protect cannot handle.
# Pre-open ports before the assert-err lambda.
(let [p-bad-offset (port/open seek-test-path :read)]
  (let [[ok? _] (protect ((fn () (port/seek p-bad-offset "not-an-int"))))]
    (assert (not ok?) "port/seek with non-integer offset returns error"))
  (port/close p-bad-offset))

(let [p-bad-from (port/open seek-test-path :read)]
  (let [[ok? _] (protect ((fn () (port/seek p-bad-from 0 :from :bogus))))]
    (assert (not ok?) "port/seek with invalid :from value returns error"))
  (port/close p-bad-from))

(let [p-incomplete (port/open seek-test-path :read)]
  (let [[ok? _] (protect ((fn () (port/seek p-incomplete 0 :from))))]
    (assert (not ok?) "port/seek with incomplete :from pair returns error"))
  (port/close p-incomplete))

# --- Cleanup ---
(subprocess/system "rm" ["-f" seek-test-path])

# ==============================
# Encoding-aware port/read
# ==============================

# Text port read returns string
(def enc-type-path (path/join scratch "enc-type"))
(spit enc-type-path "hello")
(let [ts (port/read (port/open enc-type-path :read) 5)
      tb (port/read (port/open-bytes enc-type-path :read) 5)]
  (assert (string? ts) "text port read returns string")
  (assert (bytes? tb) "binary port read returns bytes")
  (assert (= ts "hello") "text port read content correct"))

# Multi-byte UTF-8 characters: reading N returns N characters
(def enc-mb-path (path/join scratch "enc-mb"))
(spit enc-mb-path "héllo")
(let [result (port/read (port/open enc-mb-path :read) 6)]
  (assert (string? result) "multibyte text read returns string")
  (assert (= result "héllo") "multibyte text read content correct"))

# Binary port read of same multi-byte file returns bytes
(let [result (port/read (port/open-bytes enc-mb-path :read) 6)]
  (assert (bytes? result) "binary port read of multibyte file returns bytes"))

# port/read-all on text port returns string
(def enc-all-path (path/join scratch "enc-all"))
(spit enc-all-path "hello world")
(let [result (port/read-all (port/open enc-all-path :read))]
  (assert (string? result) "text port read-all returns string")
  (assert (= result "hello world") "text port read-all content correct"))

# port/read-all on binary port returns bytes
(let [result (port/read-all (port/open-bytes enc-all-path :read))]
  (assert (bytes? result) "binary port read-all returns bytes"))

# --- Cleanup: the whole scratch tree, covering every fixture above ---
(file/delete-dir-all scratch)
