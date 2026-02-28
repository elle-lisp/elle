## AWS Signature Version 4 Implementation
## Elle version — idiomatic translation of sigv4.scm
##
## Exercises: string manipulation, bytes type, crypto primitives,
## uri-encode, higher-order functions, recursive patterns.

## ============================================================================
## DateTime Functions
## ============================================================================

## Pad integer to width with leading zeros
(defn pad-int (n width)
  (letrec ((pad (fn (s)
                  (if (>= (length s) width)
                    s
                    (pad (append "0" s))))))
    (pad (number->string n))))

## Parse simplified ISO 8601 timestamp
## Format: 2023-02-08T15:30:45Z
## Returns: (year month day hour minute second)
(defn parse-timestamp-simple (timestamp-str)
  (let ((year   (string->int (substring timestamp-str 0 4)))
        (month  (string->int (substring timestamp-str 5 7)))
        (day    (string->int (substring timestamp-str 8 10)))
        (hour   (string->int (substring timestamp-str 11 13)))
        (minute (string->int (substring timestamp-str 14 16)))
        (second (string->int (substring timestamp-str 17 19))))
    (list year month day hour minute second)))

## Format timestamp as AWS date (YYYYMMDD)
(defn format-aws-date (year month day)
  (string-join (list (pad-int year 4)
                     (pad-int month 2)
                     (pad-int day 2))
               ""))

## Format timestamp as AWS datetime (YYYYMMDDTHHMMSSZ)
(defn format-aws-datetime (year month day hour minute second)
  (string-join (list (pad-int year 4)
                     (pad-int month 2)
                     (pad-int day 2)
                     "T"
                     (pad-int hour 2)
                     (pad-int minute 2)
                     (pad-int second 2)
                     "Z")
               ""))

## ============================================================================
## AWS SigV4 Components
## ============================================================================

## Create canonical headers string
## Headers are alist: ((name . value) ...)
## Output: name:value\n for each, lowercase, trimmed
(defn canonical-headers-string (headers)
  (string-join
    (map (fn (header)
           (string-join (list (string-downcase (first header))
                              ":"
                              (string-trim (rest header))
                              "\n")
                        ""))
         headers)
    ""))

## Get semicolon-separated list of signed header names (lowercase, sorted)
(defn signed-headers-list (headers)
  (string-join
    (map (fn (h) (string-downcase (first h))) headers)
    ";"))

## Create canonical query string
## Params are alist: ((key . value) ...)
## Output: key=value&key=value (no trailing &)
(defn canonical-query-string (params)
  (if (empty? params)
    ""
    (string-join
      (map (fn (p) (string-join (list (first p) "=" (rest p)) ""))
           params)
      "&")))

## ============================================================================
## Full SigV4 Signing (with real crypto)
## ============================================================================

## Create the canonical request string
(defn canonical-request (method uri query-params headers payload)
  (let* ((canonical-headers (canonical-headers-string headers))
         (signed-headers (signed-headers-list headers))
         (payload-hash (bytes->hex (crypto/sha256 payload))))
    (string-join (list method "\n"
                       uri "\n"
                       (canonical-query-string query-params) "\n"
                       canonical-headers "\n"
                       signed-headers "\n"
                       payload-hash)
                 "")))

## Create the string to sign
(defn string-to-sign (datetime scope canonical-req)
  (string-join (list "AWS4-HMAC-SHA256" "\n"
                     datetime "\n"
                     scope "\n"
                     (bytes->hex (crypto/sha256 canonical-req)))
               ""))

## Derive the signing key
## kSecret -> kDate -> kRegion -> kService -> kSigning
(defn derive-signing-key (secret-key date region service)
  (let* ((k-secret (string-join (list "AWS4" secret-key) ""))
         (k-date    (crypto/hmac-sha256 k-secret date))
         (k-region  (crypto/hmac-sha256 k-date region))
         (k-service (crypto/hmac-sha256 k-region service))
         (k-signing (crypto/hmac-sha256 k-service "aws4_request")))
    k-signing))

## Compute the final signature
(defn compute-signature (signing-key string-to-sign)
  (bytes->hex (crypto/hmac-sha256 signing-key string-to-sign)))

## ============================================================================
## Test Cases
## ============================================================================

(defn test-timestamp-parsing ()
  (display "=== Timestamp Parsing Test ===")
  (newline)
  (let ((ts "2023-02-08T15:30:45Z"))
    (display "Input: ")
    (display ts)
    (newline)
    (let ((parsed (parse-timestamp-simple ts)))
      (display "Parsed: ")
      (display parsed)
      (newline))))

(defn test-uri-encoding ()
  (display "=== URI Encoding Test ===")
  (newline)
  (each s (list "hello world" "path/to/resource" "special chars")
    (display "Input:   ")
    (display s)
    (newline)
    (display "Encoded: ")
    (display (uri-encode s))
    (newline)
    (newline)))

(defn test-datetime-formatting ()
  (display "=== DateTime Formatting Test ===")
  (newline)
  (let ((date (format-aws-date 2023 2 8)))
    (display "Date (YYYYMMDD): ")
    (display date)
    (newline))
  (let ((datetime (format-aws-datetime 2023 2 8 15 30 45)))
    (display "DateTime (YYYYMMDDTHHmmSSZ): ")
    (display datetime)
    (newline)))

(defn test-hex-conversion ()
  (display "=== Hex Conversion Test ===")
  (newline)
  (let ((b (bytes 72 101 108 108)))
    (display "Bytes as hex: ")
    (display (bytes->hex b))
    (newline)))

(defn test-crypto ()
  (display "=== Crypto Test ===")
  (newline)
  ## SHA-256 of empty string
  (display "SHA-256(\"\"):     ")
  (display (bytes->hex (crypto/sha256 "")))
  (newline)
  ## SHA-256 of "hello"
  (display "SHA-256(\"hello\"): ")
  (display (bytes->hex (crypto/sha256 "hello")))
  (newline)
  ## HMAC-SHA256
  (display "HMAC-SHA256(\"key\", \"message\"): ")
  (display (bytes->hex (crypto/hmac-sha256 "key" "message")))
  (newline))

(defn test-sigv4-signing ()
  (display "=== SigV4 Signing Test ===")
  (newline)
  ## AWS test case parameters
  (let* ((method "GET")
         (uri "/")
         (query-params (list (cons "Action" "ListUsers")
                             (cons "Version" "2010-05-08")))
         (headers (list (cons "content-type" "application/x-www-form-urlencoded; charset=utf-8")
                        (cons "host" "iam.amazonaws.com")
                        (cons "x-amz-date" "20150830T123600Z")))
         (payload "")
         (secret-key "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY")
         (date "20150830")
         (region "us-east-1")
         (service "iam")
         (scope (string-join (list date "/" region "/" service "/aws4_request") ""))
         (datetime "20150830T123600Z"))

    ## Build canonical request
    (def canon-req (canonical-request method uri query-params headers payload))
    (display "Canonical Request:")
    (newline)
    (display canon-req)
    (newline)
    (newline)

    ## Build string to sign
    (def sts (string-to-sign datetime scope canon-req))
    (display "String to Sign:")
    (newline)
    (display sts)
    (newline)
    (newline)

    ## Derive signing key and compute signature
    (def signing-key (derive-signing-key secret-key date region service))
    (def signature (compute-signature signing-key sts))
    (display "Signature: ")
    (display signature)
    (newline)))

## ============================================================================
## Main
## ============================================================================

(display "=== AWS Signature Version 4 Demo (Elle) ===")
(newline)
(newline)

(test-timestamp-parsing)
(newline)

(test-uri-encoding)

(test-datetime-formatting)
(newline)

(test-hex-conversion)
(newline)

(test-crypto)
(newline)

(test-sigv4-signing)
(newline)

(display "=== Complete ===")
(newline)
