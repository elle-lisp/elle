(elle/epoch 8)
## lib/dns.lisp — Pure Elle DNS client (RFC 1035)
##
## Loaded via: (def dns ((import-file "lib/dns.lisp")))
## Usage:      (dns:resolve "example.com")
##
## Async/uring-first: all I/O goes through udp/send-to and udp/recv-from,
## which dispatch through whatever I/O backend the scheduler uses.

## ── Constants ─────────────────────────────────────────────────────────

(def TYPE-A     1)
(def TYPE-AAAA  28)
(def TYPE-CNAME 5)
(def CLASS-IN   1)

# DNS header flags
(def FLAG-RD    256)   # Recursion Desired (bit 8)
(def FLAG-QR  32768)   # Query/Response (bit 15)

# RCODE values (low 4 bits of flags word 2)
(def RCODE-OK       0)
(def RCODE-FORMERR  1)
(def RCODE-SERVFAIL 2)
(def RCODE-NXDOMAIN 3)
(def RCODE-NOTIMP   4)
(def RCODE-REFUSED  5)

(def rcode-names
  {0 "ok" 1 "format-error" 2 "server-failure"
   3 "nxdomain" 4 "not-implemented" 5 "refused"})

(def MAX-CNAME-DEPTH 8)
(def DEFAULT-TIMEOUT 3000)
(def DEFAULT-RETRIES 2)

## ── Byte packing helpers ──────────────────────────────────────────────

(defn u16->bytes [n]
  "Encode a 16-bit integer as 2 big-endian bytes."
  (bytes (bit/and (bit/shr n 8) 0xff)
         (bit/and n 0xff)))

(defn read-u16 [buf offset]
  "Read a 16-bit big-endian integer from buf at offset."
  (bit/or (bit/shl (get buf offset) 8)
          (get buf (+ offset 1))))

(defn read-u32 [buf offset]
  "Read a 32-bit big-endian integer from buf at offset."
  (bit/or
    (bit/or (bit/shl (get buf offset) 24)
            (bit/shl (get buf (+ offset 1)) 16))
    (bit/or (bit/shl (get buf (+ offset 2)) 8)
            (get buf (+ offset 3)))))

## ── Domain name encoding / decoding ───────────────────────────────────

(defn encode-name [name]
  "Encode a domain name as DNS wire format (length-prefixed labels + null).
   'example.com' → (bytes 7 'example' 3 'com' 0)"
  (let [labels (string/split name ".")]
    (fold (fn [acc label]
            (let [label-bytes (bytes label)]
              (when (> (length label-bytes) 63)
                (error {:error :dns-error
                        :reason :label-too-long
                        :label label
                        :length (length label-bytes)
                        :limit 63
                        :message (concat "label too long: " label)}))
              (concat acc (bytes (length label-bytes)) label-bytes)))
          (bytes)
          (concat labels [""]))))

(defn decode-name [buf offset]
  "Decode a DNS wire-format name starting at offset.
   Returns {:name string :offset next-byte-after-name}.
   Handles compression pointers (0xC0 | offset)."
  (def @parts @[])
  (def @pos offset)
  (def @jumped false)
  (def @return-offset nil)
  (def @safety 0)
  (forever
    (when (>= safety 128)
      (error {:error :dns-format-error
              :reason :decode-loop
              :iterations safety
              :limit 128
              :message "name decode loop exceeded 128 iterations"}))
    (assign safety (+ safety 1))
    (let [b (get buf pos)]
      (cond
        # Null terminator — end of name
        ((= b 0)
         (unless jumped (assign return-offset (+ pos 1)))
         (break nil))

        # Compression pointer: top 2 bits = 11
        ((= (bit/and b 0xc0) 0xc0)
         (unless jumped
           (assign return-offset (+ pos 2)))
         (assign jumped true)
         (assign pos (bit/or (bit/shl (bit/and b 0x3f) 8)
                             (get buf (+ pos 1)))))

        # Regular label
        (true
         (let [label-len b]
           (push parts (string (slice buf (+ pos 1) (+ pos 1 label-len))))
           (assign pos (+ pos 1 label-len)))))))
  {:name (string/join (freeze parts) ".")
   :offset (or return-offset (+ pos 1))})

## ── Query building ────────────────────────────────────────────────────

(defn build-query [id name qtype]
  "Build a DNS query packet.
   id: 16-bit transaction ID
   name: domain name string
   qtype: query type (TYPE-A, TYPE-AAAA, etc.)"
  (let [header (concat (u16->bytes id)         # ID
                        (u16->bytes FLAG-RD)     # Flags: RD=1
                        (u16->bytes 1)           # QDCOUNT=1
                        (u16->bytes 0)           # ANCOUNT=0
                        (u16->bytes 0)           # NSCOUNT=0
                        (u16->bytes 0))         # ARCOUNT=0
        question (concat (encode-name name)
                          (u16->bytes qtype)
                          (u16->bytes CLASS-IN))]
    (concat header question)))

## ── Response parsing ──────────────────────────────────────────────────

(defn parse-header [buf]
  "Parse a DNS response header (first 12 bytes).
   Returns {:id :qr :opcode :aa :tc :rd :ra :rcode
            :qdcount :ancount :nscount :arcount}."
  (when (< (length buf) 12)
    (error {:error :dns-format-error
            :reason :short-header
            :length (length buf)
            :minimum 12
            :message "response too short for header"}))
  (let [flags (read-u16 buf 2)]
    {:id      (read-u16 buf 0)
     :qr      (not (= 0 (bit/and flags FLAG-QR)))
     :opcode  (bit/and (bit/shr flags 11) 0xf)
     :aa      (not (= 0 (bit/and flags 1024)))
     :tc      (not (= 0 (bit/and flags 512)))
     :rd      (not (= 0 (bit/and flags FLAG-RD)))
     :ra      (not (= 0 (bit/and flags 128)))
     :rcode   (bit/and flags 0xf)
     :qdcount (read-u16 buf 4)
     :ancount (read-u16 buf 6)
     :nscount (read-u16 buf 8)
     :arcount (read-u16 buf 10)}))

(defn skip-questions [buf offset count]
  "Skip 'count' question records starting at offset. Returns new offset."
  (if (<= count 0)
    offset
    (let* [decoded (decode-name buf offset)
           after-name decoded:offset
           # Skip QTYPE (2 bytes) + QCLASS (2 bytes)
           next (+ after-name 4)]
      (skip-questions buf next (- count 1)))))

(defn format-ipv4 [buf offset]
  "Format 4 bytes at offset as dotted-decimal IPv4 address."
  (string/join
    (list (string (get buf offset))
          (string (get buf (+ offset 1)))
          (string (get buf (+ offset 2)))
          (string (get buf (+ offset 3))))
    "."))

(defn format-ipv6 [buf offset]
  "Format 16 bytes at offset as colon-separated IPv6 address (full form)."
  (string/join
    (map (fn [i] (number->string (read-u16 buf (+ offset (* i 2))) 16))
         (range 8))
    ":"))

(defn parse-records [buf offset count]
  "Parse 'count' resource records starting at offset.
   Returns {:records [...] :offset next-offset}."
  (def @records @[])
  (def @pos offset)
  (def @i 0)
  (while (< i count)
    (let* [name-result (decode-name buf pos)
           name name-result:name
           pos2 name-result:offset
           rtype  (read-u16 buf pos2)
           rclass (read-u16 buf (+ pos2 2))
           ttl    (read-u32 buf (+ pos2 4))
           rdlen  (read-u16 buf (+ pos2 8))
           rdata-start (+ pos2 10)
           rdata-end   (+ rdata-start rdlen)]
      (let [record
              (case rtype
                TYPE-A
                (begin
                  (when (not (= rdlen 4))
                    (error {:error :dns-format-error
                            :reason :bad-rdata-length
                            :rtype :a
                            :expected 4
                            :actual rdlen
                            :message "A record rdata length is not 4"}))
                  {:type :a :name name :addr (format-ipv4 buf rdata-start) :ttl ttl})

                TYPE-AAAA
                (begin
                  (when (not (= rdlen 16))
                    (error {:error :dns-format-error
                            :reason :bad-rdata-length
                            :rtype :aaaa
                            :expected 16
                            :actual rdlen
                            :message "AAAA record rdata length is not 16"}))
                  {:type :aaaa :name name :addr (format-ipv6 buf rdata-start) :ttl ttl})

                TYPE-CNAME
                (let [cname-result (decode-name buf rdata-start)]
                  {:type :cname :name name :target cname-result:name :ttl ttl})

                nil)]
        (when record (push records record)))
      (assign pos rdata-end))
    (assign i (+ i 1)))
  {:records (freeze records) :offset pos})

(defn parse-response [buf]
  "Parse a complete DNS response packet.
   Returns {:header {...} :answers [...] :authority [...] :additional [...]}."
  (let* [header (parse-header buf)
         after-questions (skip-questions buf 12 header:qdcount)
         answers   (parse-records buf after-questions header:ancount)
         authority (parse-records buf answers:offset header:nscount)
         additional (parse-records buf authority:offset header:arcount)]
    {:header header
     :answers answers:records
     :authority authority:records
     :additional additional:records}))

## ── resolv.conf parsing ───────────────────────────────────────────────

(defn parse-resolv-conf [text]
  "Parse /etc/resolv.conf and return a list of nameserver IP strings."
  (let* [lines   (map string/trim (string/split text "\n"))
         ns-lines (filter (fn [l] (string/starts-with? l "nameserver")) lines)
         addrs   (map (fn [l]
                    (let [parts (string/split l " ")]
                      (when (>= (length parts) 2) (string/trim (parts 1)))))
                  ns-lines)]
    (freeze (filter (fn [a] (and a (not (empty? a)))) addrs))))

(defn read-nameservers []
  "Read nameserver list from /etc/resolv.conf. Returns array of IP strings.
   Falls back to [\"127.0.0.1\"] if file is missing or empty."
  (let [[ok? content] (protect (slurp "/etc/resolv.conf"))]
    (if ok?
      (let [servers (parse-resolv-conf content)]
        (if (empty? servers)
          ["127.0.0.1"]
          servers))
      ["127.0.0.1"])))

## ── Transaction ID generation ─────────────────────────────────────────

(def @next-txid 1)

(defn gen-txid []
  "Generate a monotonically increasing 16-bit transaction ID."
  (let [id next-txid]
    (assign next-txid (bit/and (+ id 1) 0xffff))
    id))

## ── DNS query execution ───────────────────────────────────────────────

(defn do-query [server name qtype timeout]
  "Send a single DNS query and return the parsed response.
   Signals :dns-timeout on timeout, :dns-error on protocol errors."
  (let* [txid (gen-txid)
         packet (build-query txid name qtype)
         sock (udp/bind "0.0.0.0" 0)]
    (defer (port/close sock)
      (udp/send-to sock packet server 53 :timeout timeout)
      (let* [[ok? result] (protect (udp/recv-from sock 512 :timeout timeout))]
        (unless ok?
          (error {:error :dns-timeout
                  :reason :query-timeout
                  :server server
                  :name name
                  :message (concat "timeout querying " server " for " name)}))
        (let* [resp-buf result:data
               resp (parse-response resp-buf)]
          # Verify transaction ID
          (unless (= resp:header:id txid)
            (error {:error :dns-error
                    :reason :txid-mismatch
                    :expected txid
                    :actual resp:header:id
                    :message "transaction ID mismatch"}))
          # Check truncation
          (when resp:header:tc
            (error {:error :dns-error
                    :reason :truncated
                    :message "response truncated (TC bit set)"}))
          # Check RCODE
          (unless (= resp:header:rcode RCODE-OK)
            (let [rcode-name (or (get rcode-names resp:header:rcode)
                                  (string resp:header:rcode))]
              (error {:error :dns-error
                      :reason :server-error
                      :rcode resp:header:rcode
                      :rcode-name rcode-name
                      :name name
                      :server server
                      :message (concat "server returned " rcode-name
                                       " for " name)})))
          resp)))))

(defn query-with-retries [server name qtype timeout retries]
  "Query with retries. Returns parsed response or signals error."
  (def @last-err nil)
  (def @attempt 0)
  (while (< attempt retries)
    (let [[ok? result] (protect (do-query server name qtype timeout))]
      (if ok?
        (break result)
        (begin
          (assign last-err result)
          (assign attempt (+ attempt 1))))))
  # All retries exhausted
  (when last-err (error last-err))
  (error {:error :dns-timeout
          :reason :retries-exhausted
          :name name
          :retries retries
          :message (concat "retries exhausted for " name)}))

## ── High-level resolver ───────────────────────────────────────────────

(defn resolve-type [name qtype server timeout retries]
  "Resolve a name to records of a specific type, following CNAMEs."
  (def @current-name name)
  (def @depth 0)
  (def @all-records @[])
  (forever
    (when (>= depth MAX-CNAME-DEPTH)
      (error {:error :dns-error
              :reason :cname-too-deep
              :name name
              :depth depth
              :limit MAX-CNAME-DEPTH
              :message (concat "CNAME chain too deep for " name)}))
    (let* [resp (query-with-retries server current-name qtype timeout retries)
           answers resp:answers
           # Collect direct answers of the requested type
           direct (filter (fn [r] (= r:type (case qtype
                                                    TYPE-A :a
                                                    TYPE-AAAA :aaaa
                                                    nil)))
                           answers)
           # Check for CNAME redirects
           cnames (filter (fn [r] (= r:type :cname)) answers)]
      (if (not (empty? direct))
        # Found direct answers — done
        (begin
          (each r in direct (push all-records r))
          (break nil))
        # Follow CNAME if present
        (if (not (empty? cnames))
          (begin
            (each r in cnames (push all-records r))
            (assign current-name (get (first cnames) :target))
            (assign depth (+ depth 1)))
          # No answers and no CNAMEs — done
          (break nil)))))
  (freeze all-records))

(defn resolve [name &named server timeout retries]
  "Resolve a domain name. Returns a list of record structs.
   Queries for both A and AAAA records.
   Options:
     :server  — nameserver IP (default: from /etc/resolv.conf)
     :timeout — per-query timeout in ms (default: 3000)
     :retries — retry count per query (default: 2)"
  (let* [srv (or server (first (read-nameservers)))
         tmo (or timeout DEFAULT-TIMEOUT)
         ret (or retries DEFAULT-RETRIES)
         a-records (let [[ok? result] (protect (resolve-type name TYPE-A srv tmo ret))]
                      (if ok? result ()))
         aaaa-records (let [[ok? result] (protect (resolve-type name TYPE-AAAA srv tmo ret))]
                         (if ok? result ()))]
    (concat a-records aaaa-records)))

(defn query [name qtype &named server timeout retries]
  "Low-level: send a single DNS query and return the full parsed response.
   qtype is an integer (1=A, 28=AAAA, 5=CNAME, etc.).
   Options:
     :server  — nameserver IP (default: from /etc/resolv.conf)
     :timeout — per-query timeout in ms (default: 3000)
     :retries — retry count per query (default: 2)"
  (let* [srv (or server (first (read-nameservers)))
         tmo (or timeout DEFAULT-TIMEOUT)
         ret (or retries DEFAULT-RETRIES)]
    (query-with-retries srv name qtype tmo ret)))

## ── Internal tests (pure, no network) ─────────────────────────────────

(defn run-internal-tests []
  "Sanity checks on wire-format helpers. Called via (dns:test)."

  # ── u16 encoding ──
  (assert (= (u16->bytes 0)     (bytes 0 0))      "u16->bytes 0")
  (assert (= (u16->bytes 256)   (bytes 1 0))      "u16->bytes 256")
  (assert (= (u16->bytes 0x1234) (bytes 0x12 0x34)) "u16->bytes 0x1234")
  (assert (= (u16->bytes 65535) (bytes 0xff 0xff)) "u16->bytes 65535")

  # ── u16 decoding ──
  (assert (= (read-u16 (bytes 0 0) 0)     0)      "read-u16 0")
  (assert (= (read-u16 (bytes 1 0) 0)     256)    "read-u16 256")
  (assert (= (read-u16 (bytes 0x12 0x34) 0) 0x1234) "read-u16 0x1234")
  (assert (= (read-u16 (bytes 0 0 0xff 0xff) 2) 65535) "read-u16 offset")

  # ── u16 roundtrip ──
  (assert (= (read-u16 (u16->bytes 12345) 0) 12345) "u16 roundtrip")

  # ── u32 decoding ──
  (assert (= (read-u32 (bytes 0 0 0 0) 0) 0)        "read-u32 0")
  (assert (= (read-u32 (bytes 0 0 1 0) 0) 256)      "read-u32 256")
  (assert (= (read-u32 (bytes 0 0 0xe 0x10) 0) 3600) "read-u32 3600")

  # ── name encoding ──
  (let [enc (encode-name "example.com")]
    (assert (= (get enc 0) 7)               "encode-name: first label length")
    (assert (= (string (slice enc 1 8)) "example") "encode-name: first label")
    (assert (= (get enc 8) 3)               "encode-name: second label length")
    (assert (= (string (slice enc 9 12)) "com")    "encode-name: second label")
    (assert (= (get enc 12) 0)              "encode-name: null terminator"))

  # ── name decode (no compression) ──
  (let* [encoded (encode-name "www.example.com")
         result  (decode-name encoded 0)]
    (assert (= result:name "www.example.com") "decode-name: simple")
    (assert (= result:offset (length encoded)) "decode-name: offset past name"))

  # ── name decode (with compression pointer) ──
  # Build a buffer: [7 "example" 3 "com" 0] then [3 "www" 0xC0 0x00]
  # The pointer 0xC0 0x00 points to offset 0 = "example.com"
  (let* [base (encode-name "example.com")
         ptr-name (concat (bytes 3) (bytes "www") (bytes 0xc0 0x00))
         buf (concat base ptr-name)
         result (decode-name buf (length base))]
    (assert (= result:name "www.example.com")   "decode-name: compression")
    (assert (= result:offset (length buf))      "decode-name: compression offset"))

  # ── query building ──
  (let [q (build-query 0x1234 "example.com" TYPE-A)]
    # Header: 12 bytes
    (assert (= (read-u16 q 0) 0x1234) "build-query: txid")
    (assert (= (read-u16 q 2) FLAG-RD) "build-query: flags RD")
    (assert (= (read-u16 q 4) 1)       "build-query: qdcount")
    (assert (= (read-u16 q 6) 0)       "build-query: ancount")
    # Question section starts at offset 12
    (assert (= (get q 12) 7)           "build-query: name label length"))

  # ── header parsing ──
  # Construct a minimal response header: QR=1, RD=1, RA=1, RCODE=0
  (let* [flags (bit/or FLAG-QR (bit/or FLAG-RD 128))  # QR + RD + RA
         header (concat (u16->bytes 0xabcd)    # ID
                         (u16->bytes flags)     # Flags
                         (u16->bytes 1)         # QDCOUNT
                         (u16->bytes 2)         # ANCOUNT
                         (u16->bytes 0)         # NSCOUNT
                         (u16->bytes 0))       # ARCOUNT
         parsed (parse-header header)]
    (assert (= parsed:id 0xabcd)        "parse-header: id")
    (assert parsed:qr                   "parse-header: qr")
    (assert parsed:rd                   "parse-header: rd")
    (assert parsed:ra                   "parse-header: ra")
    (assert (not parsed:tc)             "parse-header: tc=false")
    (assert (not parsed:aa)             "parse-header: aa=false")
    (assert (= parsed:rcode 0)          "parse-header: rcode")
    (assert (= parsed:qdcount 1)        "parse-header: qdcount")
    (assert (= parsed:ancount 2)        "parse-header: ancount"))

  # ── IPv4 formatting ──
  (assert (= (format-ipv4 (bytes 93 184 216 34) 0)  "93.184.216.34")
    "format-ipv4")
  (assert (= (format-ipv4 (bytes 127 0 0 1) 0)      "127.0.0.1")
    "format-ipv4 loopback")

  # ── IPv6 formatting ──
  (assert (= (format-ipv6 (bytes 0x20 0x01 0x0d 0xb8 0 0 0 0 0 0 0 0 0 0 0 1) 0)
             "2001:db8:0:0:0:0:0:1")
    "format-ipv6")

  # ── resolv.conf parsing ──
  (assert (= (parse-resolv-conf "nameserver 8.8.8.8\nnameserver 8.8.4.4\n")
             ["8.8.8.8" "8.8.4.4"])
    "parse-resolv-conf: two servers")

  (assert (= (parse-resolv-conf "# comment\nnameserver 1.1.1.1\nsearch example.com\n")
             ["1.1.1.1"])
    "parse-resolv-conf: with comment and search")

  (assert (empty? (parse-resolv-conf ""))
    "parse-resolv-conf: empty")

  # ── Full response parsing (synthetic A record) ──
  # Build a complete DNS response for "example.com" → 93.184.216.34
  (let* [txid 0x1234
         flags (bit/or FLAG-QR (bit/or FLAG-RD 128))
         header (concat (u16->bytes txid)
                         (u16->bytes flags)
                         (u16->bytes 1)    # QDCOUNT
                         (u16->bytes 1)    # ANCOUNT
                         (u16->bytes 0)    # NSCOUNT
                         (u16->bytes 0))  # ARCOUNT
         qname (encode-name "example.com")
         question (concat qname
                           (u16->bytes TYPE-A)
                           (u16->bytes CLASS-IN))
         # Answer: compression pointer to offset 12 (qname in question)
         answer (concat (bytes 0xc0 12)          # Name pointer
                         (u16->bytes TYPE-A)       # TYPE
                         (u16->bytes CLASS-IN)     # CLASS
                         (bytes 0 0 0xe 0x10)      # TTL = 3600
                         (u16->bytes 4)            # RDLENGTH
                         (bytes 93 184 216 34))   # RDATA
         packet (concat header question answer)
         resp (parse-response packet)]
    (assert (= resp:header:id txid)               "full parse: txid")
    (assert resp:header:qr                        "full parse: qr")
    (assert (= (length resp:answers) 1)           "full parse: 1 answer")
    (let [a (first resp:answers)]
      (assert (= a:type :a)                       "full parse: type A")
      (assert (= a:name "example.com")            "full parse: name")
      (assert (= a:addr "93.184.216.34")          "full parse: addr")
      (assert (= a:ttl 3600)                      "full parse: ttl")))

  # ── Full response parsing (synthetic AAAA record) ──
  (let* [txid 0x5678
         flags (bit/or FLAG-QR (bit/or FLAG-RD 128))
         header (concat (u16->bytes txid)
                         (u16->bytes flags)
                         (u16->bytes 1)    # QDCOUNT
                         (u16->bytes 1)    # ANCOUNT
                         (u16->bytes 0)
                         (u16->bytes 0))
         qname (encode-name "example.com")
         question (concat qname
                           (u16->bytes TYPE-AAAA)
                           (u16->bytes CLASS-IN))
         answer (concat (bytes 0xc0 12)           # Name pointer
                         (u16->bytes TYPE-AAAA)     # TYPE
                         (u16->bytes CLASS-IN)      # CLASS
                         (bytes 0 0 0x0e 0x10)      # TTL = 3600
                         (u16->bytes 16)            # RDLENGTH = 16
                         (bytes 0x26 0x06 0x28 0x00 0x02 0x20 0x00 0x01
                                0x02 0x48 0x18 0x93 0x25 0xc8 0x19 0x46))
         packet (concat header question answer)
         resp (parse-response packet)]
    (assert (= (length resp:answers) 1)            "aaaa parse: 1 answer")
    (let [a (first resp:answers)]
      (assert (= a:type :aaaa)                     "aaaa parse: type")
      (assert (= a:addr "2606:2800:220:1:248:1893:25c8:1946")
        "aaaa parse: addr")))

  # ── CNAME + A response ──
  (let* [txid 0x9999
         flags (bit/or FLAG-QR (bit/or FLAG-RD 128))
         header (concat (u16->bytes txid)
                         (u16->bytes flags)
                         (u16->bytes 1)    # QDCOUNT
                         (u16->bytes 2)    # ANCOUNT (CNAME + A)
                         (u16->bytes 0)
                         (u16->bytes 0))
         qname (encode-name "www.example.com")
         question (concat qname
                           (u16->bytes TYPE-A)
                           (u16->bytes CLASS-IN))
         # CNAME answer
         cname-target (encode-name "example.com")
         cname-answer (concat (bytes 0xc0 12)        # Name pointer
                               (u16->bytes TYPE-CNAME)
                               (u16->bytes CLASS-IN)
                               (bytes 0 0 0 60)        # TTL = 60
                               (u16->bytes (length cname-target))
                               cname-target)
         # A answer for the CNAME target
         a-answer (concat (encode-name "example.com")
                           (u16->bytes TYPE-A)
                           (u16->bytes CLASS-IN)
                           (bytes 0 0 0xe 0x10)        # TTL = 3600
                           (u16->bytes 4)
                           (bytes 93 184 216 34))
         packet (concat header question cname-answer a-answer)
         resp (parse-response packet)]
    (assert (= (length resp:answers) 2) "cname+a: 2 answers")
    (let [cname-rec (first resp:answers)
          a-rec (first (rest resp:answers))]
      (assert (= cname-rec:type :cname)           "cname+a: first is cname")
      (assert (= cname-rec:target "example.com")  "cname+a: target")
      (assert (= a-rec:type :a)                   "cname+a: second is A")
      (assert (= a-rec:addr "93.184.216.34")      "cname+a: addr")))

  true)

## ── Exports ───────────────────────────────────────────────────────────

(fn []
  {:resolve        resolve
   :query          query
   :parse-response parse-response
   :build-query    build-query

   # Constants
   :TYPE-A         TYPE-A
   :TYPE-AAAA      TYPE-AAAA
   :TYPE-CNAME     TYPE-CNAME
   :CLASS-IN       CLASS-IN

   # Testing
   :test           run-internal-tests})
