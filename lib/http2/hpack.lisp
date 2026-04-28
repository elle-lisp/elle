(elle/epoch 9)
## lib/http2/hpack.lisp — HPACK header compression (RFC 7541)
##
## Loaded via:
##   (def huffman ((import "std/http2/huffman")))
##   (def hpack   ((import "std/http2/hpack") :huffman huffman))
##
## Exports: {:make-encoder :make-decoder :encode :decode :test}

(fn [&named huffman]

  ## ── Static table (RFC 7541 Appendix A) ─────────────────────────────────
  ## 61 entries, 1-indexed. Index 0 is unused.
  ## Each entry is [name value].

  (def static-table
    [nil  # index 0: unused
      [":authority" ""] [":method" "GET"] [":method" "POST"] [":path" "/"]
     [":path" "/index.html"] [":scheme" "http"] [":scheme" "https"]
     [":status" "200"] [":status" "204"] [":status" "206"] [":status" "304"]
     [":status" "400"] [":status" "404"] [":status" "500"] ["accept-charset" ""]
     ["accept-encoding" "gzip, deflate"] ["accept-language" ""]
     ["accept-ranges" ""] ["accept" ""] ["access-control-allow-origin" ""]
     ["age" ""] ["allow" ""] ["authorization" ""] ["cache-control" ""]
     ["content-disposition" ""] ["content-encoding" ""] ["content-language" ""]
     ["content-length" ""] ["content-location" ""] ["content-range" ""]
     ["content-type" ""] ["cookie" ""] ["date" ""] ["etag" ""] ["expect" ""]
     ["expires" ""] ["from" ""] ["host" ""] ["if-match" ""]
     ["if-modified-since" ""] ["if-none-match" ""] ["if-range" ""]
     ["if-unmodified-since" ""] ["last-modified" ""] ["link" ""] ["location" ""]
     ["max-forwards" ""] ["proxy-authenticate" ""] ["proxy-authorization" ""]
     ["range" ""] ["referer" ""] ["refresh" ""] ["retry-after" ""] ["server" ""]
     ["set-cookie" ""] ["strict-transport-security" ""] ["transfer-encoding" ""]
     ["user-agent" ""] ["vary" ""] ["via" ""] ["www-authenticate" ""]])
  (def STATIC-TABLE-SIZE 61)

  ## ── Static table reverse lookup ────────────────────────────────────────

  (defn build-static-index []
    "Build name→index and name+value→index maps for fast lookup."
    (let [by-name @{}
          by-pair @{}]
      (def @i 1)
      (while (<= i STATIC-TABLE-SIZE)
        (let* [entry (get static-table i)
               name (get entry 0)
               value (get entry 1)
               pair-key (concat name "\x00" value)]
          (when (nil? (get by-name name)) (put by-name name i))  # Store first occurrence for name+value pair
          (when (nil? (get by-pair pair-key)) (put by-pair pair-key i)))
        (assign i (+ i 1)))
      {:by-name (freeze by-name) :by-pair (freeze by-pair)}))
  (def static-index (build-static-index))

  ## ── Dynamic table ──────────────────────────────────────────────────────
  ## FIFO with size accounting. Newest entries at front (index 0).
  ## HPACK index = static-table-size + dynamic-index + 1

  (defn make-dynamic-table [max-size]
    "Create a dynamic table with given max size. Returns mutable struct."
    @{:entries @[] :size 0 :max-size max-size})
  (defn dt-entry-size [name value]
    "Size of a dynamic table entry: 32 + name-length + value-length."
    (+ 32 (string/size-of name) (string/size-of value)))
  (defn dt-evict [dt]
    "Evict entries from the dynamic table until size <= max-size."
    (while (> dt:size dt:max-size)
      (let* [entries dt:entries
             last-idx (- (length entries) 1)
             entry (get entries last-idx)
             name (get entry 0)
             value (get entry 1)
             entry-sz (dt-entry-size name value)]
        (remove entries last-idx)
        (put dt :size (- dt:size entry-sz)))))
  (defn dt-add [dt name value]
    "Add an entry to the dynamic table, evicting as needed."
    (let [entry-sz (dt-entry-size name value)]
      (let [entries dt:entries]
        (insert entries 0 [name value]))
      (put dt :size (+ dt:size entry-sz))
      (dt-evict dt)))
  (defn dt-set-max-size [dt new-max]
    "Update the dynamic table max size and evict if necessary."
    (put dt :max-size new-max)
    (dt-evict dt))
  (defn dt-lookup [dt name value]
    "Look up a header in static + dynamic tables.
     Returns {:index i :exact? bool} or nil."
    (block (let [pair-key (concat name "\x00" value)]
             (let [exact-static (get static-index:by-pair pair-key)]
               (when (not (nil? exact-static))
                 (break {:index exact-static :exact? true})))  # Check dynamic table
             (def @dyn-name-idx nil)
             (def @dyn-exact nil)
             (def @j 0)
             (let [entries dt:entries]
               (while (< j (length entries))
                 (let* [entry (get entries j)
                        ename (get entry 0)
                        evalue (get entry 1)]
                   (when (= ename name)
                     (when (nil? dyn-name-idx)
                       (assign dyn-name-idx (+ STATIC-TABLE-SIZE j 1)))
                     (when (= evalue value)
                       (assign dyn-exact (+ STATIC-TABLE-SIZE j 1)))))
                 (assign j (+ j 1))))  # Return exact dynamic match if found
             (when (not (nil? dyn-exact))
               (break {:index dyn-exact :exact? true}))  # Name-only match in static table
             (let [name-static (get static-index:by-name name)]
               (when (not (nil? name-static))
                 (break {:index name-static :exact? false})))  # Name-only match in dynamic table
             (when (not (nil? dyn-name-idx))
               (break {:index dyn-name-idx :exact? false}))
             nil)))
  (defn dt-get [dt index]
    "Get [name value] from the combined static+dynamic table by HPACK index.
     Index 1-61 = static, 62+ = dynamic."
    (cond
      (<= index 0) (error {:error :h2-error
                           :reason :compression-error
                           :message "HPACK: invalid index 0"})
      (<= index STATIC-TABLE-SIZE) (get static-table index)
      true
        (let* [dyn-idx (- index STATIC-TABLE-SIZE 1)
               entries dt:entries]
          (when (>= dyn-idx (length entries))
            (error {:error :h2-error
                    :reason :compression-error
                    :message (concat "HPACK: dynamic table index out of range: "
                      (string index))}))
          (get entries dyn-idx))))

  ## ── Variable-length integer codec (RFC 7541 Section 5.1) ──────────────

  (defn encode-int [value prefix-bits]
    "Encode an integer with the given prefix bit count. Returns list of byte values."
    (let [max-prefix (- (bit/shl 1 prefix-bits) 1)]
      (if (< value max-prefix)
        [value]
        (let [result @[max-prefix]
              @v (- value max-prefix)]
          (while (>= v 128)
            (push result (bit/or (bit/and v 0x7f) 0x80))
            (assign v (bit/shr v 7)))
          (push result v)
          (freeze result)))))
  (defn decode-int [buf offset prefix-bits]
    "Decode a variable-length integer. Returns {:value int :offset next-offset}."
    (let* [max-prefix (- (bit/shl 1 prefix-bits) 1)
           first-byte (bit/and (get buf offset) max-prefix)]
      (if (< first-byte max-prefix)
        {:value first-byte :offset (+ offset 1)}
        (let [@value max-prefix
              @shift 0
              @pos (+ offset 1)]
          (forever
            (when (>= pos (length buf))
              (error {:error :h2-error
                      :reason :compression-error
                      :message "HPACK: truncated integer"}))
            (let [b (get buf pos)]
              (assign value (+ value (bit/shl (bit/and b 0x7f) shift)))
              (assign shift (+ shift 7))
              (assign pos (+ pos 1))
              (when (= 0 (bit/and b 0x80)) (break nil))))
          {:value value :offset pos}))))

  ## ── String codec (RFC 7541 Section 5.2) ────────────────────────────────

  (defn encode-string [s use-huffman?]
    "Encode an HPACK string literal. Returns bytes."
    (let [str-bytes (if (string? s) (bytes s) s)]
      (if (and use-huffman? huffman)
        (let* [encoded (huffman:encode str-bytes)
               len-ints (encode-int (length encoded) 7)
               first-byte (bit/or 0x80 (get len-ints 0))]
          (concat (apply bytes first-byte (rest len-ints)) encoded))
        (let* [len-ints (encode-int (length str-bytes) 7)
               first-byte (bit/and 0x7f (get len-ints 0))]
          (concat (apply bytes first-byte (rest len-ints)) str-bytes)))))
  (defn decode-string [buf offset]
    "Decode an HPACK string literal. Returns {:value string :offset next-offset}."
    (let* [huffman? (not (= 0 (bit/and (get buf offset) 0x80)))
           int-result (decode-int buf offset 7)
           str-len int-result:value
           str-start int-result:offset
           str-end (+ str-start str-len)]
      (when (> str-end (length buf))
        (error {:error :h2-error
                :reason :compression-error
                :message "HPACK: truncated string"}))
      (let [raw (slice buf str-start str-end)]
        {:value (if (and huffman? huffman)
                  (string (huffman:decode raw))
                  (string raw))
         :offset str-end})))

  ## ── Encoder ────────────────────────────────────────────────────────────

  (defn make-encoder [&named @table-size @use-huffman]
    "Create an HPACK encoder. Returns mutable encoder state."
    (default table-size 4096)
    (default use-huffman true)
    @{:table (make-dynamic-table table-size) :huffman use-huffman})
  (defn hpack-encode [encoder headers]
    "Encode a list of [name value] pairs into an HPACK header block. Returns bytes."
    (let [dt encoder:table
          use-huff encoder:huffman
          parts @[]]
      (each hdr in headers
        (let* [name (get hdr 0)
               value (get hdr 1)
               lookup (dt-lookup dt name value)]
          (cond  # Exact match — indexed header field (Section 6.1)
            (and lookup lookup:exact?)
              (let* [idx-ints (encode-int lookup:index 7)
                     first-byte (bit/or 0x80 (get idx-ints 0))]
                (push parts (apply bytes first-byte (rest idx-ints))))

              # Name match — literal with incremental indexing (Section 6.2.1)
              (and lookup (not lookup:exact?))
              (let* [idx-ints (encode-int lookup:index 6)
                     first-byte (bit/or 0x40 (get idx-ints 0))]
                (push parts (apply bytes first-byte (rest idx-ints)))
                (push parts (encode-string value use-huff))
                (dt-add dt name value))

              # No match — literal with incremental indexing, new name
              true
              (begin
                (push parts (bytes 0x40))
                (push parts (encode-string name use-huff))
                (push parts (encode-string value use-huff))
                (dt-add dt name value)))))
      (apply concat (freeze parts))))

  ## ── Decoder ────────────────────────────────────────────────────────────

  (defn make-decoder [&named @table-size]
    "Create an HPACK decoder. Returns mutable decoder state."
    (default table-size 4096)
    @{:table (make-dynamic-table table-size)})
  (defn hpack-decode [decoder buf]
    "Decode an HPACK header block. Returns a list of [name value] pairs."
    (let [dt decoder:table
          result @[]
          @offset 0]
      (while (< offset (length buf))
        (let [b (get buf offset)]
          (cond  # 1xxxxxxx: Indexed header field (Section 6.1)
            (not (= 0 (bit/and b 0x80)))
              (let [int-res (decode-int buf offset 7)]
                (let [[name value] (dt-get dt int-res:value)]
                  (push result [name value])
                  (assign offset int-res:offset)))

              # 01xxxxxx: Literal with incremental indexing (Section 6.2.1)
              (not (= 0 (bit/and b 0x40)))
              (let [int-res (decode-int buf offset 6)]
                (if (= int-res:value 0)  # New name
                  (let* [name-res (decode-string buf int-res:offset)
                         value-res (decode-string buf name-res:offset)]
                    (push result [name-res:value value-res:value])
                    (dt-add dt name-res:value value-res:value)
                    (assign offset value-res:offset))  # Indexed name
                  (let* [[name _] (dt-get dt int-res:value)
                         value-res (decode-string buf int-res:offset)]
                    (push result [name value-res:value])
                    (dt-add dt name value-res:value)
                    (assign offset value-res:offset))))

              # 001xxxxx: Dynamic table size update (Section 6.3)
              (not (= 0 (bit/and b 0x20)))
              (let [int-res (decode-int buf offset 5)]
                (dt-set-max-size dt int-res:value)
                (assign offset int-res:offset))

              # 0001xxxx: Literal never indexed (Section 6.2.3)
              (not (= 0 (bit/and b 0x10)))
              (let [int-res (decode-int buf offset 4)]
                (if (= int-res:value 0)
                  (let* [name-res (decode-string buf int-res:offset)
                         value-res (decode-string buf name-res:offset)]
                    (push result [name-res:value value-res:value])
                    (assign offset value-res:offset))
                  (let* [[name _] (dt-get dt int-res:value)
                         value-res (decode-string buf int-res:offset)]
                    (push result [name value-res:value])
                    (assign offset value-res:offset))))

              # 0000xxxx: Literal without indexing (Section 6.2.2)
              true
              (let [int-res (decode-int buf offset 4)]
                (if (= int-res:value 0)
                  (let* [name-res (decode-string buf int-res:offset)
                         value-res (decode-string buf name-res:offset)]
                    (push result [name-res:value value-res:value])
                    (assign offset value-res:offset))
                  (let* [[name _] (dt-get dt int-res:value)
                         value-res (decode-string buf int-res:offset)]
                    (push result [name value-res:value])
                    (assign offset value-res:offset)))))))
      (freeze result)))

  ## ── Tests ──────────────────────────────────────────────────────────────

  (defn run-tests []  # ── Variable-length integer codec ──
    (assert (= (encode-int 10 5) [10]) "encode-int: 10 in 5-bit prefix")
    (assert (= (encode-int 31 5) [31 0]) "encode-int: 31 in 5-bit prefix")
    (assert (= (encode-int 1337 5) [31 154 10])
      "encode-int: 1337 in 5-bit prefix")

    # Roundtrip
    (each [val prefix] in [[0 5] [10 5] [30 5] [31 5] [127 7] [128 7] [255 8]
                           [1337 5] [65535 4]]
      (let* [encoded (encode-int val prefix)
             buf (apply bytes encoded)
             decoded (decode-int buf 0 prefix)]
        (assert (= decoded:value val)
          (concat "int roundtrip: " (string val) "/" (string prefix)))))

    # ── Static table ──
    (assert (= (get (get static-table 1) 0) ":authority")
      "static table: index 1")
    (assert (= (get (get static-table 2) 0) ":method") "static table: index 2")
    (assert (= (get (get static-table 2) 1) "GET") "static table: index 2 value")

    # ── Dynamic table ──
    (let [dt (make-dynamic-table 256)]
      (dt-add dt "custom-key" "custom-value")
      (assert (= dt:size (+ 32 10 12)) "dt: size after add")
      (assert (= (length dt:entries) 1) "dt: 1 entry")
      (let [entry (get dt:entries 0)]
        (assert (= (get entry 0) "custom-key") "dt: name")
        (assert (= (get entry 1) "custom-value") "dt: value")))

    # Dynamic table eviction
    (let [dt (make-dynamic-table 64)]
      (dt-add dt "a" "b")  # 32 + 1 + 1 = 34
      (assert (= (length dt:entries) 1) "dt evict: 1 entry")
      (dt-add dt "c" "d")  # 34 + 34 = 68 > 64, evict first
      (assert (= (length dt:entries) 1) "dt evict: still 1 after eviction")
      (assert (= (get (get dt:entries 0) 0) "c") "dt evict: newest entry"))

    # ── dt-lookup ──
    (let [dt (make-dynamic-table 4096)]
      (let [res (dt-lookup dt ":method" "GET")]
        (assert (= res:index 2) "lookup: :method GET = static 2")
        (assert res:exact? "lookup: exact"))  # Static name-only match
      (let [res (dt-lookup dt ":authority" "example.com")]
        (assert (= res:index 1) "lookup: :authority name match")
        (assert (not res:exact?) "lookup: not exact"))  # Dynamic match
      (dt-add dt "custom-key" "custom-value")
      (let [res (dt-lookup dt "custom-key" "custom-value")]
        (assert (= res:index 62) "lookup: dynamic exact match")
        (assert res:exact? "lookup: dynamic exact")))

    # ── String codec ──
    # Without Huffman
    (let* [encoded (encode-string "custom-key" false)
           decoded (decode-string encoded 0)]
      (assert (= decoded:value "custom-key") "string roundtrip (no huff)"))

    # With Huffman (if available)
    (when huffman
      (let* [encoded (encode-string "www.example.com" true)
             decoded (decode-string encoded 0)]
        (assert (= decoded:value "www.example.com") "string roundtrip (huffman)")))

    # ── RFC 7541 Appendix C.2.1: Literal Header Field with Indexing ──
    # Custom header: custom-key: custom-value
    (let* [encoder (make-encoder :use-huffman false)
           decoder (make-decoder)
           encoded (hpack-encode encoder [["custom-key" "custom-value"]])
           decoded (hpack-decode decoder encoded)]
      (assert (= (length decoded) 1) "C.2.1: one header")
      (assert (= (get (get decoded 0) 0) "custom-key") "C.2.1: name")
      (assert (= (get (get decoded 0) 1) "custom-value") "C.2.1: value"))

    # ── Encode/decode roundtrip with multiple headers ──
    (let* [encoder (make-encoder :use-huffman false)
           decoder (make-decoder)
           headers [["custom-key" "custom-value"] [":method" "GET"]
                    [":path" "/"] [":scheme" "https"]
                    [":authority" "example.com"]]
           encoded (hpack-encode encoder headers)
           decoded (hpack-decode decoder encoded)]
      (assert (= (length decoded) (length headers))
        "multi-header roundtrip: count")
      (def @k 0)
      (while (< k (length headers))
        (assert (= (get (get decoded k) 0) (get (get headers k) 0))
          (concat "multi-header roundtrip: name " (string k)))
        (assert (= (get (get decoded k) 1) (get (get headers k) 1))
          (concat "multi-header roundtrip: value " (string k)))
        (assign k (+ k 1))))

    # ── Dynamic table shared state across requests ──
    (let* [encoder (make-encoder :use-huffman false)
           decoder (make-decoder)  # First request
           hdrs1 [["custom-key" "custom-value"]]
           enc1 (hpack-encode encoder hdrs1)
           dec1 (hpack-decode decoder enc1)  # Second request — same header should be indexed
           enc2 (hpack-encode encoder hdrs1)
           dec2 (hpack-decode decoder enc2)]
      (assert (= (length dec1) 1) "shared state: first decode")  ## Second encoding should be shorter via indexing
      (assert (= (length dec2) 1) "shared state: second decode")
      (assert (< (length enc2) (length enc1))
        "shared state: second encoding shorter"))

    # ── RFC 7541 C.2.4: Indexed Header Field ──
    # :method GET is static index 2
    (let* [encoder (make-encoder :use-huffman false)
           decoder (make-decoder)
           encoded (hpack-encode encoder [[":method" "GET"]])
           decoded (hpack-decode decoder encoded)]
      (assert (= (length decoded) 1) "C.2.4: one header")
      (let [hdr (get decoded 0)]
        (assert (= (get hdr 0) ":method") "C.2.4: name")
        (assert (= (get hdr 1) "GET") "C.2.4: value"))
      (assert (= (length encoded) 1) "C.2.4: single byte")
      (assert (= (get encoded 0) 0x82) "C.2.4: byte is 0x82"))
    true)

  ## ── Table size update ─────────────────────────────────────────────────

  (defn set-encoder-table-size [encoder new-max]
    "Update the encoder's dynamic table max size and evict as needed.
     Called when peer sends SETTINGS_HEADER_TABLE_SIZE."
    (dt-set-max-size encoder:table new-max))

  ## ── Exports ────────────────────────────────────────────────────────────

  {:make-encoder make-encoder
   :make-decoder make-decoder
   :encode hpack-encode
   :decode hpack-decode
   :encode-int encode-int
   :decode-int decode-int
   :set-encoder-table-size set-encoder-table-size
   :static-table static-table
   :test run-tests})
