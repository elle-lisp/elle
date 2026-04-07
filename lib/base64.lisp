## lib/base64.lisp — Base64 encoding/decoding (pure Elle)
##
## Implements RFC 4648 standard and URL-safe alphabets.
##
## Usage:
##   (def b64 ((import "std/base64")))
##   (b64:encode "hello")       => "aGVsbG8="
##   (b64:decode "aGVsbG8=")    => (bytes "hello")
##   (b64:encode-url "hello")   => "aGVsbG8"
##   (b64:decode-url "aGVsbG8") => (bytes "hello")

(fn []

  (def std-chars "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/")
  (def url-chars "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_")

  (defn build-decode-table [chars]
    (let [[tbl @[]] [cb (bytes chars)]]
      (each _ in (range 256) (push tbl -1))
      (each i in (range 64) (put tbl (cb i) i))
      (freeze tbl)))

  (def std-decode (build-decode-table std-chars))
  (def url-decode (build-decode-table url-chars))

  (defn strip-padding [s]
    "Remove trailing '=' characters from a base64 string."
    (var end (length s))
    (while (and (> end 0) (= (s (dec end)) "="))
      (assign end (dec end)))
    (slice s 0 end))

  ## ── Encode ───────────────────────────────────────────────────────

  (defn encode-with [data chars pad?]
    (let* [[input (bytes data)]
           [len   (length input)]
           [acc   @""]]
      (var i 0)
      (while (<= (+ i 2) (dec len))
        (let [[a (input i)] [b (input (inc i))] [c (input (+ i 2))]]
          (append acc (chars (bit/shr a 2)))
          (append acc (chars (bit/or (bit/shl (bit/and a 3) 4) (bit/shr b 4))))
          (append acc (chars (bit/or (bit/shl (bit/and b 15) 2) (bit/shr c 6))))
          (append acc (chars (bit/and c 63))))
        (assign i (+ i 3)))
      (match (- len i)
        [1 (let [[a (input i)]]
             (append acc (chars (bit/shr a 2)))
             (append acc (chars (bit/shl (bit/and a 3) 4)))
             (when pad? (append acc "==")))]
        [2 (let [[a (input i)] [b (input (inc i))]]
             (append acc (chars (bit/shr a 2)))
             (append acc (chars (bit/or (bit/shl (bit/and a 3) 4) (bit/shr b 4))))
             (append acc (chars (bit/shl (bit/and b 15) 2)))
             (when pad? (append acc "=")))]
        [_ nil])
      (freeze acc)))

  (defn encode [data]     "Base64-encode (standard, padded)."   (encode-with data std-chars true))
  (defn encode-url [data] "Base64-encode (URL-safe, no pad)."   (encode-with data url-chars false))

  ## ── Decode ───────────────────────────────────────────────────────

  (defn decode-with [data table]
    (unless (string? data)
      (error {:error :type-error
              :message (string "base64: expected string, got " (type-of data))}))
    (let* [[s     (strip-padding (string/trim data))]
           [slen  (length s)]
           [input (bytes s)]
           [acc   @[]]
           [lookup (fn [pos]
                     (let [[v (table (input pos))]]
                       (when (= v -1)
                         (error {:error :base64-error
                                 :message (string "base64/decode: invalid char at " pos)}))
                       v))]]
      (var i 0)
      (while (<= (+ i 3) (dec slen))
        (let [[a (lookup i)] [b (lookup (inc i))] [c (lookup (+ i 2))] [d (lookup (+ i 3))]]
          (push acc (bit/or (bit/shl a 2) (bit/shr b 4)))
          (push acc (bit/and (bit/or (bit/shl b 4) (bit/shr c 2)) 255))
          (push acc (bit/and (bit/or (bit/shl c 6) d) 255)))
        (assign i (+ i 4)))
      (match (- slen i)
        [0 nil]
        [2 (let [[a (lookup i)] [b (lookup (inc i))]]
             (push acc (bit/or (bit/shl a 2) (bit/shr b 4))))]
        [3 (let [[a (lookup i)] [b (lookup (inc i))] [c (lookup (+ i 2))]]
             (push acc (bit/or (bit/shl a 2) (bit/shr b 4)))
             (push acc (bit/and (bit/or (bit/shl b 4) (bit/shr c 2)) 255)))]
        [_ (error {:error :base64-error :message "base64/decode: invalid length"})])
      (freeze (bytes ;acc))))

  (defn decode [data]     "Base64-decode (standard)."   (decode-with data std-decode))
  (defn decode-url [data] "Base64-decode (URL-safe)."   (decode-with data url-decode))

  {:encode encode :decode decode :encode-url encode-url :decode-url decode-url})
