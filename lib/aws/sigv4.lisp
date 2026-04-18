(elle/epoch 7)
## lib/aws/sigv4.lisp — AWS Signature Version 4 signing
##
## Pure Elle. Receives crypto and jiff functions as arguments.
##
## Usage:
##   (def crypto (import-file "target/debug/libelle_crypto.so"))
##   (def jiff   (import-file "target/debug/libelle_jiff.so"))
##   (def sigv4 ((import-file "lib/aws/sigv4.lisp") crypto jiff))
##   (sigv4:sign "GET" "/" nil nil "s3.us-east-1.amazonaws.com" creds "us-east-1" "s3")

(fn [crypto jiff]
  (def sha256      (get crypto :sha256))
  (def hmac-sha256 (get crypto :hmac-sha256))
  (def ts-now      (get jiff :timestamp))
  (def ts-format   (get jiff :temporal/format))

  # ==========================================================================
  # Internal helpers
  # ==========================================================================

  (defn payload-hash [body]
    "SHA-256 hex digest of the request body (empty string if nil)."
    (bytes->hex (sha256 (or body ""))))

  (defn canonical-headers [pairs]
    "Canonical headers string from sorted [name value] pairs."
    (string/join
      (map (fn [[name value]]
             (string/join [(string/lowercase name) ":"
                           (string/trim value) "\n"] ""))
           pairs)
      ""))

  (defn signed-header-names [pairs]
    "Semicolon-separated lowercase header names."
    (string/join (map (fn [[name _]] (string/lowercase name)) pairs) ";"))

  (defn canonical-query [params]
    "Canonical query string from sorted [key value] pairs, or nil/empty."
    (if (or (nil? params) (empty? params))
      ""
      (string/join (map (fn [[k v]] (concat k "=" v)) params) "&")))

  (defn canonical-request [method path query pairs body-hash]
    "Build the canonical request string."
    (string/join [method "\n"
                  path "\n"
                  (canonical-query query) "\n"
                  (canonical-headers pairs) "\n"
                  (signed-header-names pairs) "\n"
                  body-hash]
                 ""))

  (defn string-to-sign [datetime scope creq]
    "Build the string-to-sign."
    (string/join ["AWS4-HMAC-SHA256\n"
                  datetime "\n"
                  scope "\n"
                  (bytes->hex (sha256 creq))]
                 ""))

  (defn derive-key [secret date region service]
    "Derive the signing key via HMAC chain."
    (-> (concat "AWS4" secret)
        (hmac-sha256 date)
        (hmac-sha256 region)
        (hmac-sha256 service)
        (hmac-sha256 "aws4_request")))

  # ==========================================================================
  # Public API
  # ==========================================================================

  (defn sign-headers [method path query body host creds region service]
    "Sign an AWS request and return all required headers as a struct.

     Returns struct with :host :x-amz-date :x-amz-content-sha256
     :authorization, and optionally :x-amz-security-token."
    (let* [datetime  (ts-format "%Y%m%dT%H%M%SZ" (ts-now))
           date      (slice datetime 0 8)
           hash      (payload-hash body)
           ## Build sorted [name value] pairs for canonical request
           base      [["host" host]
                       ["x-amz-content-sha256" hash]
                       ["x-amz-date" datetime]]
           pairs     (if (nil? creds:session-token)
                        base
                        (sort (concat base
                                      [["x-amz-security-token"
                                        creds:session-token]])))
           ## Sign
           scope     (string/join [date region service "aws4_request"] "/")
           creq      (canonical-request method path query pairs hash)
           sts       (string-to-sign datetime scope creq)
           key       (derive-key creds:secret-key date region service)
           sig       (bytes->hex (hmac-sha256 key sts))
           auth      (string/join ["AWS4-HMAC-SHA256 Credential="
                                    creds:access-key "/" scope
                                    ", SignedHeaders="
                                    (signed-header-names pairs)
                                    ", Signature=" sig]
                                   "")
           ## Build result struct
           result    {:host                  host
                       :x-amz-date            datetime
                       :x-amz-content-sha256  hash
                       :authorization         auth}]
      (if (nil? creds:session-token)
        result
        (merge result {:x-amz-security-token creds:session-token}))))

  {:sign sign-headers})
