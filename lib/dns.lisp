(elle/epoch 13)
# audited: 2026-09-28
## A DNS client (RFC 1035) in pure Elle: nameservers, queries with retries, and CNAME chains.
## lib/overview.md
##
## Loaded via: (def dns ((import "std/dns")))
## Usage:      (dns:resolve "example.com")
##
## Async/uring-first: all I/O goes through udp/send-to and udp/recv-from,
## which dispatch through whatever I/O backend the scheduler uses. The wire
## codec lives in lib/dns/wire.lisp.

(def wire ((import "std/dns/wire")))

## ── Constants ─────────────────────────────────────────────────────────

(def TYPE-A wire:TYPE-A)
(def TYPE-AAAA wire:TYPE-AAAA)
(def TYPE-CNAME wire:TYPE-CNAME)
(def CLASS-IN wire:CLASS-IN)
(def RCODE-OK wire:RCODE-OK)
(def rcode-names wire:rcode-names)
(def build-query wire:build-query)
(def parse-response wire:parse-response)

(def MAX-CNAME-DEPTH 8)
(def DEFAULT-TIMEOUT 3000)
(def DEFAULT-RETRIES 2)

## ── resolv.conf parsing ───────────────────────────────────────────────

(defn parse-resolv-conf [text]
  "Parse the text of /etc/resolv.conf and return an array of nameserver IP
   strings."
  (let* [lines (map string/trim (string/split text "\n"))
         ns-lines (filter (fn [l] (string/starts-with? l "nameserver")) lines)
         addrs (map (fn [l]
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
        (if (empty? servers) ["127.0.0.1"] servers))
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
    (defer
      (port/close sock)
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
                      :message (concat "server returned " rcode-name " for "
                                       name)})))
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
           direct (filter (fn [r]
                            (= r:type
                               (case qtype
                                 TYPE-A :a
                                 TYPE-AAAA :aaaa
                                 nil))) answers)
           # Check for CNAME redirects
           cnames (filter (fn [r] (= r:type :cname)) answers)]
      # Found direct answers — done
      (if (not (empty? direct))
        (begin
          (each r in direct
            (push all-records r))
          (break nil))
        # Follow CNAME if present
        (if (not (empty? cnames))
          (begin
            (each r in cnames
              (push all-records r))
            (assign current-name (get (first cnames) :target))
            (assign depth (+ depth 1)))
          # No answers and no CNAMEs — done
          (break nil)))))
  (freeze all-records))

(defn resolve [name &named server timeout retries]
  "Resolve a domain name. Returns its record structs, the A records before
   the AAAA records. A query that fails contributes no records.
   Options:
     :server  — nameserver IP (default: from /etc/resolv.conf)
     :timeout — per-query timeout in ms (default: 3000)
     :retries — retry count per query (default: 2)"
  (let* [srv (or server (first (read-nameservers)))
         tmo (or timeout DEFAULT-TIMEOUT)
         ret (or retries DEFAULT-RETRIES)
         a-records (let [[ok? result] (protect (resolve-type name TYPE-A srv tmo
                         ret))]
                     (if ok? result ()))
         aaaa-records (let [[ok? result] (protect (resolve-type name TYPE-AAAA
                            srv tmo ret))]
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
  "Sanity checks on the codec and resolv.conf parsing. Called via
   (dns:test)."
  (wire:test)

  # ── resolv.conf parsing ──
  (assert (= (parse-resolv-conf "nameserver 8.8.8.8\nnameserver 8.8.4.4\n")
             ["8.8.8.8" "8.8.4.4"]) "parse-resolv-conf: two servers")

  (assert (= (parse-resolv-conf "# comment\nnameserver 1.1.1.1\nsearch example.com\n")
             ["1.1.1.1"]) "parse-resolv-conf: with comment and search")

  (assert (empty? (parse-resolv-conf "")) "parse-resolv-conf: empty")

  true)

## ── Exports ───────────────────────────────────────────────────────────

(fn []
  {:resolve resolve
   :query query
   :parse-response parse-response
   :build-query build-query

   # Constants
   :TYPE-A TYPE-A
   :TYPE-AAAA TYPE-AAAA
   :TYPE-CNAME TYPE-CNAME
   :CLASS-IN CLASS-IN

   # Testing
   :test run-internal-tests})
