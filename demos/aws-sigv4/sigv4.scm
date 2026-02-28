;; AWS Signature Version 4 Implementation
;; Chez Scheme version
;;
;; This demo tests:
;; - Datetime parsing and formatting (ISO 8601)
;; - String manipulation and canonicalization
;; - Cryptographic hashing (SHA256, HMAC-SHA256)
;; - URL/path encoding
;; - HTTP request signing for AWS API authentication

;; ============================================================================
;; String Utilities
;; ============================================================================

;; Find character in string
(defn char-in-string? (char str)
  (let loop ((i 0))
    (cond
      ((= i (string-length str)) #f)
      ((char=? char (string-ref str i)) #t)
      (else (loop (+ i 1))))))

;; Convert integer to hex string with padding
(defn to-hex-string (num width)
  (let ((hex-digits "0123456789ABCDEF"))
    (let loop ((n num) (result '()) (w width))
      (if (= w 0)
        (apply string-append (map string result))
         (loop (quotient n 16)
               (cons (string-ref hex-digits (modulo n 16)) result)
               (- w 1))))))

;; Convert bytevector to hex string
(defn bytevector->hex-string (bv)
  (apply string-append
    (map (lambda (b) (to-hex-string b 2))
         (bytevector->u8-list bv))))

;; ============================================================================
;; DateTime Functions
;; ============================================================================

;; Simple timestamp parser for ISO 8601
;; Format: 2023-02-08T15:30:45Z
(defn parse-timestamp-simple (timestamp-str)
  "Parse simplified ISO 8601 timestamp
    Returns: (year month day hour minute second)"
   ;; Extract parts manually
   (let ((year (string->number (substring timestamp-str 0 4)))
         (month (string->number (substring timestamp-str 5 7)))
         (day (string->number (substring timestamp-str 8 10)))
         (hour (string->number (substring timestamp-str 11 13)))
         (minute (string->number (substring timestamp-str 14 16)))
         (second (string->number (substring timestamp-str 17 19))))
     (list year month day hour minute second)))

;; Pad integer to width with zeros
(defn pad-int (n width)
  (let ((s (number->string n)))
    (string-append (make-string (max 0 (- width (string-length s))) #\0) s)))

;; Format timestamp as AWS date (YYYYMMDD)
(defn format-aws-date (year month day)
  (string-append
    (pad-int year 4)
    (pad-int month 2)
    (pad-int day 2)))

;; Format timestamp as AWS datetime (YYYYMMDDTHHMMSSZ)
(defn format-aws-datetime (year month day hour minute second)
  (string-append
    (pad-int year 4)
    (pad-int month 2)
    (pad-int day 2)
    "T"
    (pad-int hour 2)
    (pad-int minute 2)
    (pad-int second 2)
    "Z"))

;; ============================================================================
;; URL Encoding
;; ============================================================================

;; Percent-encode character
(defn percent-encode-char (c)
  (let ((code (char->integer c)))
    (string-append "%" (to-hex-string code 2))))

;; Check if character is unreserved (safe in URI)
(defn uri-unreserved? (c)
  (or (and (char>=? c #\a) (char<=? c #\z))
      (and (char>=? c #\A) (char<=? c #\Z))
      (and (char>=? c #\0) (char<=? c #\9))
      (char-in-string? c "-._~")))

;; Percent-encode string for URI component
(defn uri-encode (str)
  (apply string-append
    (map (lambda (c)
           (if (uri-unreserved? c)
             (string c)
             (percent-encode-char c)))
         (string->list str))))

;; ============================================================================
;; AWS SigV4 Components
;; ============================================================================

;; Create canonical headers string
(defn canonical-headers-string (headers)
  "Format headers in canonical form for SigV4
    Headers are: name:value\\n (lowercase, sorted)"
   (apply string-append
     (map (lambda (header)
            (let ((name (car header))
                  (value (cdr header)))
              (string-append
                (string-downcase name) ":"
                (string-trim value) "\n")))
          headers)))

;; Get list of signed header names
(defn signed-headers-list (headers)
  (apply string-append
    (let ((names (map (lambda (h) (string-downcase (car h))) headers)))
      (apply string-append
         (map string->list names)
         (cdr (apply append
           (map (lambda (h) (list ";" h))
                names)))))))

;; Create canonical query string
(defn canonical-query-string (params)
  "Format query parameters in canonical form
    Parameters should be sorted by key"
   (if (null? params)
     ""
     (apply string-append
       (map (lambda (p)
              (string-append (car p) "=" (cdr p) "&"))
            (reverse (cdr (reverse params)))))))

;; ============================================================================
;; Hash Functions (Placeholders)
;; ============================================================================

;; SHA256 (placeholder - would use FFI in real implementation)
(defn sha256 (data)
  (make-bytevector 32 0))

;; HMAC-SHA256 (placeholder - would use FFI in real implementation)
(defn hmac-sha256 (key data)
  (make-bytevector 32 0))

;; ============================================================================
;; Test Cases
;; ============================================================================

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
  (let ((test-cases (list
    "hello world"
    "path/to/resource"
    "special chars")))
    (for-each
      (lambda (s)
         (display "Input:  ")
         (display s)
         (newline)
         (display "Encoded: ")
         (display (uri-encode s))
         (newline)
         (newline))
       test-cases)))

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
  (let ((bv (make-bytevector 4)))
    (bytevector-u8-set! bv 0 72)   ; 0x48 = 'H'
    (bytevector-u8-set! bv 1 101)  ; 0x65 = 'e'
    (bytevector-u8-set! bv 2 108)  ; 0x6c = 'l'
    (bytevector-u8-set! bv 3 108)  ; 0x6c = 'l'
    (display "Bytevector as hex: ")
    (display (bytevector->hex-string bv))
    (newline)))

;; ============================================================================
;; Main
;; ============================================================================

(display "=== AWS Signature Version 4 Demo (Chez Scheme) ===")
(newline)
(newline)

(test-timestamp-parsing)
(newline)

(test-uri-encoding)
(newline)

(test-datetime-formatting)
(newline)

(test-hex-conversion)
(newline)

(display "=== Complete ===")
(newline)
(newline)
(display "Implementation Status:")
(newline)
(display "✓ Timestamp parsing (ISO 8601)")
(newline)
(display "✓ URI encoding (percent encoding)")
(newline)
(display "✓ DateTime formatting (AWS format)")
(newline)
(display "✓ Hex conversion")
(newline)
(display "✗ Cryptographic hashing (SHA256, HMAC-SHA256)")
(newline)
(display "  → Requires FFI to OpenSSL or crypto library")
(newline)
