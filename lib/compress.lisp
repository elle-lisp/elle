(elle/epoch 10)
## lib/compress.lisp — Gzip, zlib, deflate, and zstd via FFI
##
## Usage:
##   (def z ((import "std/compress")))
##   (z:gzip "hello")                => compressed bytes
##   (z:gunzip compressed)           => (bytes "hello")
##   (z:zstd "hello")                => compressed bytes
##   (z:unzstd compressed)           => (bytes "hello")

(fn []
  (def libz (ffi/native "libz.so"))
  (def libzstd (ffi/native "libzstd.so"))
  (def null-ptr (ptr/from-int 0))

  (defn cfn [lib name ret args]
    (let [p (ffi/lookup lib name)
          s (ffi/signature ret args)]
      (fn [& a] (apply ffi/call p s a))))

  ## ── zlib C bindings ──────────────────────────────────────────────

  (def z-deflateInit2
    (cfn libz "deflateInit2_" :int @[:ptr :int :int :int :int :int :string :int]))
  (def z-deflate (cfn libz "deflate" :int @[:ptr :int]))
  (def z-deflateEnd (cfn libz "deflateEnd" :int @[:ptr]))
  (def z-inflateInit2 (cfn libz "inflateInit2_" :int @[:ptr :int :string :int]))
  (def z-inflate (cfn libz "inflate" :int @[:ptr :int]))
  (def z-inflateEnd (cfn libz "inflateEnd" :int @[:ptr]))
  (def z-bound (cfn libz "compressBound" :size @[:size]))

  ## z_stream struct: 112 bytes on x86_64
  ## Offsets: next_in=0 avail_in=8 next_out=24 avail_out=32
  (def Z_STREAM_SIZE 112)
  (def Z_FINISH 4)
  (def Z_OK 0)
  (def Z_STREAM_END 1)
  (def ZLIB_VERSION "1.3.1")

  ## ── zstd C bindings ──────────────────────────────────────────────

  (def zstd-compress
    (cfn libzstd "ZSTD_compress" :size @[:ptr :size :ptr :size :int]))
  (def zstd-decompress
    (cfn libzstd "ZSTD_decompress" :size @[:ptr :size :ptr :size]))
  (def zstd-bound (cfn libzstd "ZSTD_compressBound" :size @[:size]))
  (def zstd-is-error (cfn libzstd "ZSTD_isError" :int @[:size]))
  (def zstd-err-name (cfn libzstd "ZSTD_getErrorName" :ptr @[:size]))
  (def zstd-frame-size
    (cfn libzstd "ZSTD_getFrameContentSize" :i64 @[:ptr :size]))

  ## ── Helpers ──────────────────────────────────────────────────────

  (defn ptr->bytes [ptr len]
    "Read len bytes from a C pointer into Elle bytes."
    (let [acc @[]]
      (each i in (range len)
        (push acc (ffi/read (ptr/add ptr i) :u8)))
      (freeze (bytes ;acc))))

  (defn to-input [data]
    "Coerce to bytes + pin for FFI."
    (let [b (if (string? data) (bytes data) data)]
      (ffi/pin b)))

  ## ── Deflate-based compression (gzip, zlib, raw deflate) ──────────

  (defn deflate-compress [data window-bits level]
    "Compress data using zlib's deflate with the given windowBits."
    (let* [input (bytes data)
           in-len (length input)
           out-size (+ (z-bound in-len) 64)
           stream (ffi/malloc Z_STREAM_SIZE)
           out-buf (ffi/malloc out-size)
           in-pin (ffi/pin input)]
      (each i in (range Z_STREAM_SIZE)
        (ffi/write (ptr/add stream i) :u8 0))  ## deflateInit2(stream, level, Z_DEFLATED=8, windowBits, memLevel=8, Z_DEFAULT_STRATEGY=0)
      (let [rc (z-deflateInit2 stream level 8 window-bits 8 0 ZLIB_VERSION
                               Z_STREAM_SIZE)]
        (unless (= rc Z_OK)
          (ffi/free stream)
          (ffi/free out-buf)
          (error {:error :compress-error
                  :message (string "deflateInit2 failed: " rc)})))  ## Set input
      (ffi/write stream :ptr in-pin)  # next_in = 0
      (ffi/write (ptr/add stream 8) :u32 in-len)  # avail_in = 8
      (ffi/write (ptr/add stream 24) :ptr out-buf)  # next_out = 24
      (ffi/write (ptr/add stream 32) :u32 out-size)  # avail_out = 32
      ## Compress
      (let [rc (z-deflate stream Z_FINISH)]
        (unless (= rc Z_STREAM_END)
          (z-deflateEnd stream)
          (ffi/free stream)
          (ffi/free out-buf)
          (error {:error :compress-error :message (string "deflate failed: " rc)})))

      ## Read output size: total_out is at offset 40 (after avail_out at 32 + padding)
      ## Offsets: next_out 24:8, avail_out 32:4, pad 36:4, total_out 40:8
      (let* [avail-out (ffi/read (ptr/add stream 32) :u32)
             compressed-len (- out-size avail-out)
             result (ptr->bytes out-buf compressed-len)]
        (z-deflateEnd stream)
        (ffi/free stream)
        (ffi/free out-buf)
        result)))

  (defn deflate-decompress [data window-bits]
    "Decompress data using zlib's inflate with the given windowBits."
    (let* [input (bytes data)
           in-len (length input)
           out-size (* in-len 4)  # initial guess
           stream (ffi/malloc Z_STREAM_SIZE)
           in-pin (ffi/pin input)]
      (each i in (range Z_STREAM_SIZE)
        (ffi/write (ptr/add stream i) :u8 0))
      (let [rc (z-inflateInit2 stream window-bits ZLIB_VERSION Z_STREAM_SIZE)]
        (unless (= rc Z_OK)
          (ffi/free stream)
          (error {:error :compress-error
                  :message (string "inflateInit2 failed: " rc)})))  ## Set input
      (ffi/write stream :ptr in-pin)
      (ffi/write (ptr/add stream 8) :u32 in-len)  ## Decompress in a loop, growing output buffer as needed
      (def @buf-size (max out-size 256))
      (def @out-buf (ffi/malloc buf-size))
      (def @total-out 0)
      (def @done false)
      (while (not done)
        (let [space (- buf-size total-out)]
          (ffi/write (ptr/add stream 24) :ptr (ptr/add out-buf total-out))
          (ffi/write (ptr/add stream 32) :u32 space)
          (let [rc (z-inflate stream 0)]
            (let [produced (- space (ffi/read (ptr/add stream 32) :u32))]
              (assign total-out (+ total-out produced)))
            (match rc
              1 (assign done true)  # Z_STREAM_END
              0
                (when (zero? (ffi/read (ptr/add stream 32) :u32))  ## Output buffer full, grow
                  (let* [new-size (* buf-size 2)
                         new-buf (ffi/malloc new-size)]
                    (each i in (range total-out)
                      (ffi/write (ptr/add new-buf i)
                                 :u8 (ffi/read (ptr/add out-buf i) :u8)))
                    (ffi/free out-buf)
                    (assign out-buf new-buf)
                    (assign buf-size new-size)))
              _
                (begin
                  (z-inflateEnd stream)
                  (ffi/free stream)
                  (ffi/free out-buf)
                  (error {:error :compress-error
                          :message (string "inflate failed: " rc)}))))))
      (let [result (ptr->bytes out-buf total-out)]
        (z-inflateEnd stream)
        (ffi/free stream)
        (ffi/free out-buf)
        result)))

  ## windowBits: 15+16=31 for gzip, 15 for zlib, -15 for raw deflate
  (defn gzip [data & opts]
    (deflate-compress data 31 (if (> (length opts) 0) (first opts) 6)))
  (defn gunzip [data]
    (deflate-decompress data 31))
  (defn zlib [data & opts]
    (deflate-compress data 15 (if (> (length opts) 0) (first opts) 6)))
  (defn unzlib [data]
    (deflate-decompress data 15))
  (defn deflate [data & opts]
    (deflate-compress data -15 (if (> (length opts) 0) (first opts) 6)))
  (defn inflate [data]
    (deflate-decompress data -15))

  ## ── Zstd ─────────────────────────────────────────────────────────

  (defn zstd [data & opts]
    (let* [input (bytes data)
           in-len (length input)
           level (if (> (length opts) 0) (first opts) 3)
           bound (zstd-bound in-len)
           out-buf (ffi/malloc bound)
           in-pin (ffi/pin input)
           rc (zstd-compress out-buf bound in-pin in-len level)]
      (unless (zero? (zstd-is-error rc))
        (ffi/free out-buf)
        (error {:error :compress-error
                :message (string "zstd: " (ffi/string (zstd-err-name rc)))}))
      (let [result (ptr->bytes out-buf rc)]
        (ffi/free out-buf)
        result)))

  (defn unzstd [data]
    (let* [input (bytes data)
           in-len (length input)
           in-pin (ffi/pin input)
           frame-size (zstd-frame-size in-pin in-len)]
      (when (< frame-size 0)
        (error {:error :compress-error
                :message "unzstd: cannot determine frame size"}))
      (if (zero? frame-size)
        (bytes)
        (let* [out-buf (ffi/malloc frame-size)
               rc (zstd-decompress out-buf frame-size in-pin in-len)]
          (unless (zero? (zstd-is-error rc))
            (ffi/free out-buf)
            (error {:error :compress-error
                    :message (string "unzstd: " (ffi/string (zstd-err-name rc)))}))
          (let [result (ptr->bytes out-buf rc)]
            (ffi/free out-buf)
            result)))))

  {:gzip gzip
   :gunzip gunzip
   :zlib zlib
   :unzlib unzlib
   :deflate deflate
   :inflate inflate
   :zstd zstd
   :unzstd unzstd})
