(elle/epoch 10)
## lib/uuid.lisp — UUID generation and parsing (pure Elle)
##
## Implements UUID v4 (random), parsing, nil, and version detection.
## Pass the hash plugin to enable v5 (SHA-1 name-based) support.
##
## Usage:
##   (def uuid ((import "std/uuid")))
##   (uuid:v4)                                          => "a1b2c3d4-..."
##   (uuid:parse "550E8400-E29B-41D4-A716-446655440000") => "550e8400-..."
##   (uuid:nil)                                          => "00000000-0000-..."
##   (uuid:version (uuid:v4))                            => 4
##
## With v5 support:
##   (def hash-plugin (import "plugin/hash"))
##   (def uuid ((import "std/uuid") hash-plugin))
##   (uuid:v5 "6ba7b810-9dad-11d1-80b4-00c04fd430c8" "example.com")

(fn [& opts]
  (def hex-chars "0123456789abcdef")

  (defn byte->hex [b]
    (string (hex-chars (bit/shr b 4)) (hex-chars (bit/and b 15))))

  (defn hex-char? [c]
    (or (and (>= c "0") (<= c "9")) (and (>= c "a") (<= c "f"))
        (and (>= c "A") (<= c "F"))))

  (defn hex->nibble [c]
    (if (and (>= c "0") (<= c "9"))
      (- (first (bytes c)) 48)
      (if (and (>= c "a") (<= c "f"))
        (+ (- (first (bytes c)) 97) 10)
        (if (and (>= c "A") (<= c "F"))
          (+ (- (first (bytes c)) 65) 10)
          (error {:error :uuid-error
                  :message (string "uuid: invalid hex char '" c "'")})))))

  (defn bytes->hex [b start len]
    (let [acc @""]
      (each i in (range start (+ start len))
        (append acc (byte->hex (b i))))
      (freeze acc)))

  (defn random-bytes [n]
    "Read n bytes from /dev/urandom."
    (let [p (port/open-bytes "/dev/urandom" :read)]
      (let [b (port/read p n)]
        (port/close p)
        b)))

  (defn v4 []
    "Generate a random UUID (version 4)."
    (let [b (thaw (random-bytes 16))]
      (put b 6 (bit/or (bit/and (b 6) 15) 64))  ## Set variant: byte 8 = 10xxxxxx
      (put b 8 (bit/or (bit/and (b 8) 63) 128))
      (string (bytes->hex b 0 4) "-" (bytes->hex b 4 2) "-" (bytes->hex b 6 2)
              "-" (bytes->hex b 8 2) "-" (bytes->hex b 10 6))))

  (defn parse-uuid [s]
    "Parse and normalize a UUID string to lowercase hyphenated form."
    (unless (string? s)
      (error {:error :type-error
              :message (string "uuid/parse: expected string, got " (type-of s))}))
    (let [lower (string/downcase s)]
      (unless (= (length lower) 36)
        (error {:error :uuid-error
                :message (string "uuid/parse: invalid UUID: " s)}))
      (each i in (range 36)
        (let [c (lower i)]
          (if (or (= i 8) (= i 13) (= i 18) (= i 23))
            (unless (= c "-")
              (error {:error :uuid-error
                      :message (string "uuid/parse: expected '-' at position " i)}))
            (unless (hex-char? c)
              (error {:error :uuid-error
                      :message (string "uuid/parse: invalid hex char at position "
                                       i)})))))
      lower))

  (defn uuid-nil []
    "Return the nil UUID (all zeros)."
    "00000000-0000-0000-0000-000000000000")

  (defn version [uuid-str]
    "Return the version number of a UUID, or nil for version 0."
    (unless (string? uuid-str)
      (error {:error :type-error
              :message (string "uuid/version: expected string, got "
                               (type-of uuid-str))}))
    (let [parsed (parse-uuid uuid-str)]
      (let [v (hex->nibble (parsed 14))]
        (if (zero? v) nil v))))

  ## ── v5 (requires hash plugin) ─────────────────────────────────

  (def hash-plugin (if (> (length opts) 0) (first opts) nil))

  (defn parse-hex-byte [s offset]
    (+ (* (hex->nibble (s offset)) 16) (hex->nibble (s (inc offset)))))

  (defn uuid->bytes [uuid-str]
    "Parse a UUID string into 16 raw bytes."
    (let [s (parse-uuid uuid-str)
          acc @[]]
      (each i in [0 2 4 6 9 11 14 16 19 21 24 26 28 30 32 34]
        (push acc (parse-hex-byte s i)))
      (freeze (bytes ;acc))))

  (defn v5 [namespace name]
    "Generate a deterministic UUID (version 5) from namespace UUID and name string."
    (unless hash-plugin
      (error {:error :uuid-error
              :message "uuid/v5: hash plugin required — pass it to (import \"std/uuid\")"}))
    (let* [ns-bytes (uuid->bytes namespace)
           name-bytes (bytes name)  ## Concatenate namespace + name
           input (@bytes)
           _ (each i in (range (length ns-bytes))
               (push input (ns-bytes i)))
           _ (each i in (range (length name-bytes))
               (push input (name-bytes i)))  ## SHA-1 hash
           digest (thaw (hash-plugin:sha1 (freeze input)))  ## Take first 16 bytes, set version and variant
           b (thaw (slice digest 0 16))]
      (put b 6 (bit/or (bit/and (b 6) 15) 80))  ## Set variant: byte 8 = 10xxxxxx
      (put b 8 (bit/or (bit/and (b 8) 63) 128))
      (string (bytes->hex b 0 4) "-" (bytes->hex b 4 2) "-" (bytes->hex b 6 2)
              "-" (bytes->hex b 8 2) "-" (bytes->hex b 10 6))))

  {:v4 v4 :v5 v5 :parse parse-uuid :nil uuid-nil :version version})
